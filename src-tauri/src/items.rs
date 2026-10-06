use crate::model::{Catalog, Item, ItemInput, ItemQuery};
use crate::repository::Repository;
use chrono::NaiveDate;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

pub struct ItemService {
    repository: Mutex<Repository>,
}

impl ItemService {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        Ok(Self {
            repository: Mutex::new(Repository::open(path)?),
        })
    }

    pub fn query(&self, query: &ItemQuery) -> Result<Vec<Item>, String> {
        let mut query = query.clone();
        query.due_from = normalize_date(query.due_from, "開始日")?;
        query.due_to = normalize_date(query.due_to, "終了日")?;
        validate_query(&query)?;
        query.project = clean_optional(query.project);
        query.search = clean_optional(query.search);
        self.repository.lock().map_err(lock_error)?.query(&query)
    }

    pub fn create(&self, input: ItemInput) -> Result<Item, String> {
        let input = normalize_input(input)?;
        self.repository.lock().map_err(lock_error)?.create(&input)
    }

    pub fn update(&self, id: &str, input: ItemInput) -> Result<Item, String> {
        let input = normalize_input(input)?;
        self.repository
            .lock()
            .map_err(lock_error)?
            .update(id, &input)
    }

    pub fn complete(&self, id: &str) -> Result<Item, String> {
        self.repository.lock().map_err(lock_error)?.complete(id)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.repository.lock().map_err(lock_error)?.delete(id)
    }

    pub fn catalog(&self) -> Result<Catalog, String> {
        self.repository.lock().map_err(lock_error)?.catalog()
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        self.repository.lock().map_err(lock_error)?.get_setting(key)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.repository
            .lock()
            .map_err(lock_error)?
            .set_setting(key, value)
    }
}

fn normalize_input(mut input: ItemInput) -> Result<ItemInput, String> {
    input.title = input.title.trim().to_owned();
    if input.title.is_empty() {
        return Err("タイトルを入力してください".to_owned());
    }
    input.project = clean_optional(input.project);
    input.scheduled_date = normalize_date(input.scheduled_date, "予定日")?;
    input.due_date = normalize_date(input.due_date, "締切日")?;
    let mut seen = HashSet::new();
    input.tags = input
        .tags
        .into_iter()
        .map(|tag| tag.trim().to_owned())
        .filter(|tag| !tag.is_empty() && seen.insert(tag.clone()))
        .collect();
    Ok(input)
}

fn normalize_date(value: Option<String>, label: &str) -> Result<Option<String>, String> {
    let Some(value) = clean_optional(value) else {
        return Ok(None);
    };
    let date = NaiveDate::parse_from_str(&value, "%Y-%m-%d")
        .map_err(|_| format!("{label}はYYYY-MM-DD形式で入力してください"))?;
    if date.format("%Y-%m-%d").to_string() != value {
        return Err(format!("{label}はYYYY-MM-DD形式で入力してください"));
    }
    Ok(Some(value))
}

fn validate_query(query: &ItemQuery) -> Result<(), String> {
    if let (Some(from), Some(to)) = (&query.due_from, &query.due_to) {
        if from > to {
            return Err("期限日の範囲が正しくありません".to_owned());
        }
    }
    Ok(())
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

fn lock_error(_: std::sync::PoisonError<std::sync::MutexGuard<'_, Repository>>) -> String {
    "データベースを利用できません".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ItemKind, ItemStatus, Priority};
    use tempfile::tempdir;

    fn input(kind: ItemKind, title: &str) -> ItemInput {
        ItemInput {
            kind,
            title: title.to_owned(),
            notes: String::new(),
            project: None,
            scheduled_date: None,
            due_date: None,
            priority: Priority::None,
            tags: Vec::new(),
        }
    }

    fn service() -> (tempfile::TempDir, ItemService) {
        let directory = tempdir().unwrap();
        let service = ItemService::open(directory.path().join("items.sqlite")).unwrap();
        (directory, service)
    }

    #[test]
    fn persists_crud_and_settings_across_reopen() {
        let (directory, service) = service();
        let mut task = input(ItemKind::Task, "  Prepare release  ");
        task.notes = "check changelog".to_owned();
        task.project = Some("  Studio  ".to_owned());
        task.scheduled_date = Some("2026-10-08".to_owned());
        task.due_date = Some("2026-10-12".to_owned());
        task.priority = Priority::High;
        task.tags = vec![" release ".into(), "release".into(), "review".into()];
        let created = service.create(task).unwrap();
        assert_eq!(created.title, "Prepare release");
        assert_eq!(created.tags, vec!["release", "review"]);
        service.set_setting("theme", "dark").unwrap();

        let mut edited = input(ItemKind::Bute, "Publish release");
        edited.project = Some("Studio".into());
        edited.tags = vec!["shipped".into()];
        let updated = service.update(&created.id, edited).unwrap();
        assert_eq!(updated.kind, ItemKind::Bute);
        assert_eq!(updated.created_at, created.created_at);
        assert_eq!(updated.tags, vec!["shipped"]);

        drop(service);
        let reopened = ItemService::open(directory.path().join("items.sqlite")).unwrap();
        let loaded = reopened.query(&ItemQuery::default()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title, "Publish release");
        assert_eq!(
            reopened.get_setting("theme").unwrap().as_deref(),
            Some("dark")
        );
    }

    #[test]
    fn filters_by_kind_status_project_search_and_inclusive_due_dates() {
        let (_directory, service) = service();
        let mut task = input(ItemKind::Task, "Review plan");
        task.notes = "contains needle".to_owned();
        task.project = Some("North Star".into());
        task.due_date = Some("2026-10-10".into());
        task.tags = vec!["urgent".into()];
        let task = service.create(task).unwrap();
        let mut bute = input(ItemKind::Bute, "Write plan");
        bute.project = Some("North Star".into());
        bute.due_date = Some("2026-10-11".into());
        let bute = service.create(bute).unwrap();
        service.complete(&task.id).unwrap();

        let result = service
            .query(&ItemQuery {
                kind: Some(ItemKind::Task),
                status: Some(ItemStatus::Completed),
                project: Some("North Star".into()),
                search: Some("needle".into()),
                due_from: Some("2026-10-10".into()),
                due_to: Some("2026-10-10".into()),
            })
            .unwrap();
        assert_eq!(
            result
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec![task.id.as_str()]
        );
        let active_butes = service
            .query(&ItemQuery {
                kind: Some(ItemKind::Bute),
                status: Some(ItemStatus::Active),
                ..ItemQuery::default()
            })
            .unwrap();
        assert_eq!(active_butes.len(), 1);
        assert_eq!(active_butes[0].id, bute.id);
        for search in ["Review", "North Star", "urgent"] {
            assert!(service
                .query(&ItemQuery {
                    kind: Some(ItemKind::Task),
                    search: Some(search.into()),
                    ..ItemQuery::default()
                })
                .unwrap()
                .iter()
                .any(|item| item.id == task.id));
        }
        assert!(service
            .query(&ItemQuery {
                search: Some("%_".into()),
                ..ItemQuery::default()
            })
            .unwrap()
            .is_empty());
        let all_statuses = service
            .query(&ItemQuery {
                status: None,
                ..ItemQuery::default()
            })
            .unwrap();
        assert_eq!(all_statuses.len(), 2);
        assert!(all_statuses.iter().any(|item| item.id == bute.id));
    }

    #[test]
    fn completion_is_idempotent_and_update_does_not_reopen_item() {
        let (_directory, service) = service();
        let created = service.create(input(ItemKind::Task, "Finish")).unwrap();
        let completed = service.complete(&created.id).unwrap();
        let completed_again = service.complete(&created.id).unwrap();
        assert_eq!(completed.status, ItemStatus::Completed);
        assert_eq!(completed.completed_at, completed_again.completed_at);
        let edited = service
            .update(&created.id, input(ItemKind::Task, "Finished"))
            .unwrap();
        assert_eq!(edited.status, ItemStatus::Completed);
        assert_eq!(edited.completed_at, completed.completed_at);
    }

    #[test]
    fn catalog_tracks_used_projects_and_tags_and_delete_cascades() {
        let (_directory, service) = service();
        let mut item = input(ItemKind::Task, "Clean up");
        item.project = Some("Old Project".into());
        item.tags = vec!["old-tag".into()];
        let created = service.create(item).unwrap();
        let mut changed = input(ItemKind::Task, "Clean up");
        changed.project = Some("New Project".into());
        changed.tags = vec!["new-tag".into()];
        service.update(&created.id, changed).unwrap();
        let catalog = service.catalog().unwrap();
        assert_eq!(catalog.projects, vec!["New Project"]);
        assert_eq!(catalog.tags, vec!["new-tag"]);
        service.delete(&created.id).unwrap();
        assert!(service.catalog().unwrap().projects.is_empty());
        assert!(service.catalog().unwrap().tags.is_empty());
        assert!(service.query(&ItemQuery::default()).unwrap().is_empty());
    }

    #[test]
    fn rejects_empty_titles_and_noncanonical_dates() {
        let (_directory, service) = service();
        assert!(service.create(input(ItemKind::Task, "  ")).is_err());
        let mut malformed = input(ItemKind::Task, "Valid title");
        malformed.due_date = Some("2026-2-03".into());
        assert!(service.create(malformed).is_err());
        let mut impossible = input(ItemKind::Task, "Valid title");
        impossible.scheduled_date = Some("2026-02-30".into());
        assert!(service.create(impossible).is_err());
        assert!(service
            .query(&ItemQuery {
                due_from: Some("2026-10-12".into()),
                due_to: Some("2026-10-10".into()),
                ..ItemQuery::default()
            })
            .is_err());
        assert!(service.query(&ItemQuery::default()).unwrap().is_empty());
    }
}
