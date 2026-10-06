use crate::conversations::{Conversation, Message};
use crate::model::{Catalog, Item, ItemInput, ItemKind, ItemQuery, ItemStatus, Priority};
use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};
use std::path::Path;
use uuid::Uuid;

pub(crate) struct Repository {
    connection: Connection,
}

impl Repository {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(db_error)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(db_error)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 CREATE TABLE IF NOT EXISTS projects (
                     name TEXT PRIMARY KEY
                 );
                 CREATE TABLE IF NOT EXISTS tags (
                     name TEXT PRIMARY KEY
                 );
                 CREATE TABLE IF NOT EXISTS items (
                     id TEXT PRIMARY KEY,
                     kind TEXT NOT NULL CHECK (kind IN ('task', 'bute')),
                     title TEXT NOT NULL,
                     notes TEXT NOT NULL,
                     status TEXT NOT NULL CHECK (status IN ('active', 'completed')),
                     project TEXT REFERENCES projects(name) ON DELETE SET NULL,
                     scheduled_date TEXT,
                     due_date TEXT,
                     priority TEXT NOT NULL CHECK (priority IN ('none', 'low', 'medium', 'high')),
                     created_at TEXT NOT NULL,
                     updated_at TEXT NOT NULL,
                     completed_at TEXT
                 );
                 CREATE TABLE IF NOT EXISTS item_tags (
                     item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
                     tag TEXT NOT NULL REFERENCES tags(name) ON DELETE CASCADE,
                     PRIMARY KEY (item_id, tag)
                 );
                 CREATE TABLE IF NOT EXISTS settings (
                     key TEXT PRIMARY KEY,
                     value TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS conversations (
                     id TEXT PRIMARY KEY,
                     title TEXT NOT NULL,
                     created_at TEXT NOT NULL,
                     updated_at TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS messages (
                     id TEXT PRIMARY KEY,
                     conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                     role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
                     content TEXT NOT NULL,
                     created_at TEXT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS messages_conversation_created
                     ON messages(conversation_id, created_at, id);",
            )
            .map_err(db_error)?;
        Ok(Self { connection })
    }

    pub(crate) fn query(&self, query: &ItemQuery) -> Result<Vec<Item>, String> {
        let mut sql = String::from(
            "SELECT i.id, i.kind, i.title, i.notes, i.status, i.project,
                    i.scheduled_date, i.due_date, i.priority, i.created_at,
                    i.updated_at, i.completed_at
             FROM items i WHERE 1 = 1",
        );
        let mut values = Vec::<Value>::new();
        if let Some(kind) = query.kind {
            sql.push_str(" AND i.kind = ?");
            values.push(Value::Text(kind.as_db().to_owned()));
        }
        if let Some(status) = query.status {
            sql.push_str(" AND i.status = ?");
            values.push(Value::Text(status.as_db().to_owned()));
        }
        if let Some(project) = query.project.as_deref().filter(|p| !p.is_empty()) {
            sql.push_str(" AND i.project = ?");
            values.push(Value::Text(project.to_owned()));
        }
        if let Some(search) = query.search.as_deref().filter(|s| !s.is_empty()) {
            let escaped = search
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let pattern = format!("%{escaped}%");
            sql.push_str(
                " AND (i.title LIKE ? ESCAPE '\\' COLLATE NOCASE
                      OR i.notes LIKE ? ESCAPE '\\' COLLATE NOCASE
                      OR COALESCE(i.project, '') LIKE ? ESCAPE '\\' COLLATE NOCASE
                      OR EXISTS (SELECT 1 FROM item_tags it WHERE it.item_id = i.id
                                 AND it.tag LIKE ? ESCAPE '\\' COLLATE NOCASE))",
            );
            for _ in 0..4 {
                values.push(Value::Text(pattern.clone()));
            }
        }
        if let Some(due_from) = query.due_from.as_deref() {
            sql.push_str(" AND i.due_date >= ?");
            values.push(Value::Text(due_from.to_owned()));
        }
        if let Some(due_to) = query.due_to.as_deref() {
            sql.push_str(" AND i.due_date <= ?");
            values.push(Value::Text(due_to.to_owned()));
        }
        sql.push_str(" ORDER BY i.due_date IS NULL, i.due_date ASC, i.updated_at DESC, i.id ASC");

        let mut statement = self.connection.prepare(&sql).map_err(db_error)?;
        let rows = statement
            .query_map(params_from_iter(values.iter()), read_item)
            .map_err(db_error)?;
        let mut items = Vec::new();
        for row in rows {
            let mut item = row.map_err(db_error)?;
            item.tags = self.item_tags(&item.id)?;
            items.push(item);
        }
        Ok(items)
    }

    pub(crate) fn create(&mut self, input: &ItemInput) -> Result<Item, String> {
        let tx = self.connection.transaction().map_err(db_error)?;
        if let Some(project) = input.project.as_deref() {
            tx.execute("INSERT OR IGNORE INTO projects(name) VALUES (?)", [project])
                .map_err(db_error)?;
        }
        let id = Uuid::new_v4().to_string();
        let now = timestamp();
        tx.execute(
            "INSERT INTO items (id, kind, title, notes, status, project, scheduled_date,
                                due_date, priority, created_at, updated_at, completed_at)
             VALUES (?, ?, ?, ?, 'active', ?, ?, ?, ?, ?, ?, NULL)",
            params![
                id,
                input.kind.as_db(),
                input.title,
                input.notes,
                input.project,
                input.scheduled_date,
                input.due_date,
                input.priority.as_db(),
                now,
                now,
            ],
        )
        .map_err(db_error)?;
        Self::replace_tags(&tx, &id, &input.tags)?;
        tx.commit().map_err(db_error)?;
        self.get(&id)?
            .ok_or_else(|| "項目を保存できませんでした".to_owned())
    }

    pub(crate) fn update(&mut self, id: &str, input: &ItemInput) -> Result<Item, String> {
        let tx = self.connection.transaction().map_err(db_error)?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM items WHERE id = ?)",
                [id],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if !exists {
            return Err("項目が見つかりません".to_owned());
        }
        if let Some(project) = input.project.as_deref() {
            tx.execute("INSERT OR IGNORE INTO projects(name) VALUES (?)", [project])
                .map_err(db_error)?;
        }
        tx.execute(
            "UPDATE items SET kind = ?, title = ?, notes = ?, project = ?,
                              scheduled_date = ?, due_date = ?, priority = ?, updated_at = ?
             WHERE id = ?",
            params![
                input.kind.as_db(),
                input.title,
                input.notes,
                input.project,
                input.scheduled_date,
                input.due_date,
                input.priority.as_db(),
                timestamp(),
                id,
            ],
        )
        .map_err(db_error)?;
        Self::replace_tags(&tx, id, &input.tags)?;
        Self::prune_catalog(&tx)?;
        tx.commit().map_err(db_error)?;
        self.get(id)?
            .ok_or_else(|| "項目を保存できませんでした".to_owned())
    }

    pub(crate) fn complete(&mut self, id: &str) -> Result<Item, String> {
        let tx = self.connection.transaction().map_err(db_error)?;
        let now = timestamp();
        let changed = tx
            .execute(
                "UPDATE items SET status = 'completed',
                                  completed_at = COALESCE(completed_at, ?), updated_at = ?
                 WHERE id = ?",
                params![now, now, id],
            )
            .map_err(db_error)?;
        if changed == 0 {
            return Err("項目が見つかりません".to_owned());
        }
        tx.commit().map_err(db_error)?;
        self.get(id)?
            .ok_or_else(|| "項目が見つかりません".to_owned())
    }

    pub(crate) fn delete(&mut self, id: &str) -> Result<(), String> {
        let tx = self.connection.transaction().map_err(db_error)?;
        tx.execute("DELETE FROM items WHERE id = ?", [id])
            .map_err(db_error)?;
        Self::prune_catalog(&tx)?;
        tx.commit().map_err(db_error)
    }

    pub(crate) fn catalog(&self) -> Result<Catalog, String> {
        let projects =
            self.names("SELECT name FROM projects ORDER BY name COLLATE NOCASE, name")?;
        let tags = self.names("SELECT name FROM tags ORDER BY name COLLATE NOCASE, name")?;
        Ok(Catalog { projects, tags })
    }

    pub(crate) fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        self.connection
            .query_row("SELECT value FROM settings WHERE key = ?", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(db_error)
    }

    pub(crate) fn set_setting(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO settings(key, value) VALUES (?, ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn list_conversations(&self) -> Result<Vec<Conversation>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, title, created_at, updated_at FROM conversations
             ORDER BY updated_at DESC, created_at DESC, id ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], read_conversation)
            .map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    pub(crate) fn create_conversation(&mut self) -> Result<Conversation, String> {
        let id = Uuid::new_v4().to_string();
        let now = timestamp();
        self.connection
            .execute(
                "INSERT INTO conversations(id, title, created_at, updated_at) VALUES (?, ?, ?, ?)",
                params![id, "新しい会話", now, now],
            )
            .map_err(db_error)?;
        self.conversation(&id)?
            .ok_or_else(|| "会話を保存できませんでした".to_owned())
    }

    pub(crate) fn get_messages(&self, id: &str) -> Result<Vec<Message>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, conversation_id, role, content, created_at FROM messages
             WHERE conversation_id = ? ORDER BY created_at ASC, rowid ASC",
            )
            .map_err(db_error)?;
        let rows = statement.query_map([id], read_message).map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    pub(crate) fn delete_conversation(&mut self, id: &str) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM conversations WHERE id = ?", [id])
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn append_user_message(
        &mut self,
        conversation_id: &str,
        content: &str,
        title: &str,
    ) -> Result<Vec<Message>, String> {
        let tx = self.connection.transaction().map_err(db_error)?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?)",
                [conversation_id],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        if !exists {
            return Err("会話が見つかりません".to_owned());
        }
        let is_first: bool = tx
            .query_row(
                "SELECT NOT EXISTS(SELECT 1 FROM messages WHERE conversation_id = ?)",
                [conversation_id],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        let id = Uuid::new_v4().to_string();
        let now = timestamp();
        tx.execute(
            "INSERT INTO messages(id, conversation_id, role, content, created_at) VALUES (?, ?, 'user', ?, ?)",
            params![id, conversation_id, content, now],
        ).map_err(db_error)?;
        if is_first {
            tx.execute(
                "UPDATE conversations SET title = ?, updated_at = ? WHERE id = ?",
                params![title, now, conversation_id],
            )
            .map_err(db_error)?;
        } else {
            tx.execute(
                "UPDATE conversations SET updated_at = ? WHERE id = ?",
                params![now, conversation_id],
            )
            .map_err(db_error)?;
        }
        let mut statement = tx
            .prepare(
                "SELECT id, conversation_id, role, content, created_at FROM messages
             WHERE conversation_id = ? AND id != ? ORDER BY created_at ASC, rowid ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map(params![conversation_id, id], read_message)
            .map_err(db_error)?;
        let messages = rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?;
        drop(statement);
        tx.commit().map_err(db_error)?;
        Ok(messages)
    }

    pub(crate) fn append_assistant_message(
        &mut self,
        conversation_id: &str,
        content: &str,
    ) -> Result<(), String> {
        let tx = self.connection.transaction().map_err(db_error)?;
        let id = Uuid::new_v4().to_string();
        let now = timestamp();
        let changed = tx
            .execute(
                "INSERT INTO messages(id, conversation_id, role, content, created_at)
             SELECT ?, id, 'assistant', ?, ? FROM conversations WHERE id = ?",
                params![id, content, now, conversation_id],
            )
            .map_err(db_error)?;
        if changed == 0 {
            return Err("会話が見つかりません".to_owned());
        }
        tx.execute(
            "UPDATE conversations SET updated_at = ? WHERE id = ?",
            params![now, conversation_id],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)
    }

    pub(crate) fn conversation_detail(
        &self,
        id: &str,
    ) -> Result<Option<(Conversation, Vec<Message>)>, String> {
        let Some(conversation) = self.conversation(id)? else {
            return Ok(None);
        };
        Ok(Some((conversation, self.get_messages(id)?)))
    }

    fn conversation(&self, id: &str) -> Result<Option<Conversation>, String> {
        self.connection
            .query_row(
                "SELECT id, title, created_at, updated_at FROM conversations WHERE id = ?",
                [id],
                read_conversation,
            )
            .optional()
            .map_err(db_error)
    }

    fn get(&self, id: &str) -> Result<Option<Item>, String> {
        let mut item = self
            .connection
            .query_row(
                "SELECT id, kind, title, notes, status, project, scheduled_date, due_date,
                        priority, created_at, updated_at, completed_at FROM items WHERE id = ?",
                [id],
                read_item,
            )
            .optional()
            .map_err(db_error)?;
        if let Some(item) = item.as_mut() {
            item.tags = self.item_tags(id)?;
        }
        Ok(item)
    }

    fn item_tags(&self, id: &str) -> Result<Vec<String>, String> {
        let mut statement = self
            .connection
            .prepare("SELECT tag FROM item_tags WHERE item_id = ? ORDER BY tag COLLATE NOCASE, tag")
            .map_err(db_error)?;
        let rows = statement
            .query_map([id], |row| row.get(0))
            .map_err(db_error)?;
        rows.collect::<Result<Vec<String>, _>>().map_err(db_error)
    }

    fn names(&self, sql: &str) -> Result<Vec<String>, String> {
        let mut statement = self.connection.prepare(sql).map_err(db_error)?;
        let rows = statement
            .query_map([], |row| row.get(0))
            .map_err(db_error)?;
        rows.collect::<Result<Vec<String>, _>>().map_err(db_error)
    }

    fn replace_tags(
        tx: &rusqlite::Transaction<'_>,
        id: &str,
        tags: &[String],
    ) -> Result<(), String> {
        tx.execute("DELETE FROM item_tags WHERE item_id = ?", [id])
            .map_err(db_error)?;
        for tag in tags {
            tx.execute("INSERT OR IGNORE INTO tags(name) VALUES (?)", [tag])
                .map_err(db_error)?;
            tx.execute(
                "INSERT INTO item_tags(item_id, tag) VALUES (?, ?)",
                params![id, tag],
            )
            .map_err(db_error)?;
        }
        tx.execute(
            "DELETE FROM tags WHERE NOT EXISTS (SELECT 1 FROM item_tags WHERE item_tags.tag = tags.name)",
            [],
        )
        .map_err(db_error)?;
        Ok(())
    }

    fn prune_catalog(tx: &rusqlite::Transaction<'_>) -> Result<(), String> {
        tx.execute(
            "DELETE FROM projects WHERE NOT EXISTS (SELECT 1 FROM items WHERE items.project = projects.name)",
            [],
        )
        .map_err(db_error)?;
        tx.execute(
            "DELETE FROM tags WHERE NOT EXISTS (SELECT 1 FROM item_tags WHERE item_tags.tag = tags.name)",
            [],
        )
        .map_err(db_error)?;
        Ok(())
    }
}

impl ItemKind {
    fn as_db(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Bute => "bute",
        }
    }
}

impl ItemStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
        }
    }
}

impl Priority {
    fn as_db(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

fn read_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<Item> {
    let kind: String = row.get(1)?;
    let status: String = row.get(4)?;
    let priority: String = row.get(8)?;
    Ok(Item {
        id: row.get(0)?,
        kind: match kind.as_str() {
            "task" => ItemKind::Task,
            _ => ItemKind::Bute,
        },
        title: row.get(2)?,
        notes: row.get(3)?,
        status: match status.as_str() {
            "active" => ItemStatus::Active,
            _ => ItemStatus::Completed,
        },
        project: row.get(5)?,
        scheduled_date: row.get(6)?,
        due_date: row.get(7)?,
        priority: match priority.as_str() {
            "none" => Priority::None,
            "low" => Priority::Low,
            "medium" => Priority::Medium,
            _ => Priority::High,
        },
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        completed_at: row.get(11)?,
        tags: Vec::new(),
    })
}

fn read_conversation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

fn read_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn db_error(error: rusqlite::Error) -> String {
    format!("データベースを処理できませんでした: {error}")
}
