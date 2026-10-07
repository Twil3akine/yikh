use crate::assistant_policy::{date_schema, ItemOperationPolicy};
use crate::items::ItemService;
use crate::model::{Item, ItemInput, ItemKind, ItemQuery, ItemStatus, Priority};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Default)]
pub struct AssistantTools {
    pending: Mutex<HashMap<String, PendingState>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: Value,
    pub changed: bool,
    pub pending: Option<PendingAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAction {
    pub token: String,
    pub kind: String,
    pub message: String,
    pub candidates: Vec<ActionCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionCandidate {
    pub key: String,
    pub title: String,
    pub kind: ItemKind,
    pub status: ItemStatus,
    pub notes: String,
    pub project: Option<String>,
    pub scheduled_date: Option<String>,
    pub due_date: Option<String>,
    pub priority: Priority,
}

struct PendingState {
    action: PendingAction,
    candidates: Vec<Item>,
    operation: PendingOperation,
}

#[derive(Clone)]
enum PendingOperation {
    Update(ItemPatch),
    Complete,
    ChooseDelete,
    ConfirmDelete,
}

#[derive(Clone)]
struct ItemPatch {
    title: Option<String>,
    kind: Option<ItemKind>,
    notes: Option<String>,
    project: Option<Option<String>>,
    scheduled_date: Option<Option<String>>,
    due_date: Option<Option<String>>,
    priority: Option<Priority>,
    tags: Option<Vec<String>>,
}

impl AssistantTools {
    pub fn definitions() -> Value {
        let string = |description: &str| json!({"type":"string", "description":description});
        let optional = |props: Map<String, Value>, required: Vec<&str>| json!({"type":"object", "properties":props, "required":required, "additionalProperties":false});
        let mut definitions = Vec::new();

        let mut p = Map::new();
        p.insert(
            "kind".into(),
            json!({"type":"string","enum":["task","bute"]}),
        );
        p.insert(
            "status".into(),
            json!({"type":"string","enum":["active","completed"]}),
        );
        p.insert("project".into(), string("プロジェクト名"));
        p.insert("search".into(), string("タイトル、メモ、タグの検索語"));
        p.insert("due_from".into(), string("締切日の開始日 YYYY-MM-DD"));
        p.insert("due_to".into(), string("締切日の終了日 YYYY-MM-DD"));
        p.insert(
            "scheduled_date".into(),
            string("予定日の完全一致 YYYY-MM-DD"),
        );
        p.insert("due_date".into(), string("締切日の完全一致 YYYY-MM-DD"));
        p.insert(
            "priority".into(),
            json!({"type":"string","enum":["none","low","medium","high"]}),
        );
        definitions.push(function(
            "list_items",
            "条件に合うアイテムを最新状態から検索します。",
            optional(p, vec![]),
        ));

        let mut p = Map::new();
        p.insert(
            "kind".into(),
            json!({"type":"string","enum":["task","bute"],"description":"アイテムの種類"}),
        );
        p.insert("title".into(), string("タイトル"));
        p.insert("notes".into(), string("メモ。省略時は空です"));
        p.insert(
            "project".into(),
            string("プロジェクト名。省略時は未設定です"),
        );
        p.insert("scheduled_date".into(), date_schema(false));
        p.insert("due_date".into(), date_schema(false));
        p.insert(
            "priority".into(),
            json!({"type":"string","enum":["none","low","medium","high"]}),
        );
        p.insert(
            "tags".into(),
            json!({"type":"array","items":{"type":"string"}}),
        );
        definitions.push(function(
            "create_item",
            "タイトルと種類が分かれば確認せず追加します。任意項目はユーザーが指定したものだけ渡してください。",
            optional(p, vec!["kind", "title"]),
        ));

        let mut p = Map::new();
        p.insert(
            "title".into(),
            string("現在のタイトル。完全一致で対象を指定します"),
        );
        p.insert("new_title".into(), string("変更後のタイトル"));
        p.insert(
            "kind".into(),
            json!({"type":"string","enum":["task","bute"]}),
        );
        p.insert("notes".into(), string("変更後のメモ"));
        p.insert(
            "project".into(),
            json!({"type":["string","null"],"description":"変更後のプロジェクト。nullで解除"}),
        );
        p.insert("scheduled_date".into(), date_schema(true));
        p.insert("due_date".into(), date_schema(true));
        p.insert(
            "priority".into(),
            json!({"type":"string","enum":["none","low","medium","high"]}),
        );
        p.insert(
            "tags".into(),
            json!({"type":"array","items":{"type":"string"}}),
        );
        definitions.push(function(
            "update_item",
            "一意の対象は確認せず、指定項目だけを更新します。同名の場合はアプリが候補選択を表示します。",
            optional(p, vec!["title"]),
        ));

        let mut p = Map::new();
        p.insert("title".into(), string("完了するアイテムの現在のタイトル"));
        definitions.push(function(
            "complete_item",
            "一意の対象は確認せず完了にします。同名の場合はアプリが候補選択を表示します。",
            optional(p, vec!["title"]),
        ));

        let mut p = Map::new();
        p.insert("title".into(), string("削除するアイテムの現在のタイトル"));
        definitions.push(function(
            "delete_item",
            "削除候補を表示し、ユーザー確認後に削除します。",
            optional(p, vec!["title"]),
        ));

        json!(definitions)
    }

    pub fn execute(
        &self,
        service: &ItemService,
        conversation_id: &str,
        name: &str,
        arguments: Value,
        policy: &ItemOperationPolicy,
    ) -> Result<ToolResult, String> {
        let arguments = policy.validate(name, arguments)?;
        self.execute_validated(service, conversation_id, name, arguments)
    }

    fn execute_validated(
        &self,
        service: &ItemService,
        conversation_id: &str,
        name: &str,
        arguments: Value,
    ) -> Result<ToolResult, String> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "Assistantの操作状態を利用できません".to_owned())?;
        if pending.contains_key(conversation_id) {
            return Err("先に表示中の操作を選択、確認、またはキャンセルしてください".to_owned());
        }
        match name {
            "list_items" => list_items(service, arguments),
            "create_item" => create_item(service, arguments),
            "update_item" => {
                let (title, patch) = update_arguments(arguments)?;
                let matches = exact_title_matches(service, &title)?;
                match matches.len() {
                    0 => Err("該当するアイテムが見つかりません".to_owned()),
                    1 => update_one(service, &matches[0], patch),
                    _ => {
                        let result = self.make_pending(
                            "select",
                            "更新するアイテムを選んでください。",
                            &matches,
                        );
                        store_pending(
                            &mut pending,
                            conversation_id,
                            &result,
                            matches,
                            PendingOperation::Update(patch),
                        )?;
                        Ok(result)
                    }
                }
            }
            "complete_item" | "delete_item" => {
                let args = object(arguments, &["title"])?;
                let title = required_string(&args, "title")?;
                let matches = exact_title_matches(service, &title)?;
                match matches.len() {
                    0 => Err("該当するアイテムが見つかりません".to_owned()),
                    1 if name == "complete_item" => complete_one(service, &matches[0]),
                    1 => {
                        let result = self.make_pending(
                            "delete",
                            &format!("「{}」を削除しますか？", matches[0].title),
                            &matches,
                        );
                        store_pending(
                            &mut pending,
                            conversation_id,
                            &result,
                            matches,
                            PendingOperation::ConfirmDelete,
                        )?;
                        Ok(result)
                    }
                    _ => {
                        let op = if name == "complete_item" {
                            PendingOperation::Complete
                        } else {
                            PendingOperation::ChooseDelete
                        };
                        let message = if name == "complete_item" {
                            "完了するアイテムを選んでください。"
                        } else {
                            "削除するアイテムを選んでください。"
                        };
                        let result = self.make_pending("select", message, &matches);
                        store_pending(&mut pending, conversation_id, &result, matches, op)?;
                        Ok(result)
                    }
                }
            }
            _ => Err("利用できない操作です".to_owned()),
        }
    }

    pub fn resolve(
        &self,
        service: &ItemService,
        conversation_id: &str,
        token: &str,
        candidate_key: Option<&str>,
        confirm: bool,
    ) -> Result<ToolResult, String> {
        let mut pending_map = self
            .pending
            .lock()
            .map_err(|_| "Assistantの操作状態を利用できません".to_owned())?;
        let (action, candidates, operation) = {
            let state = pending_map
                .get(conversation_id)
                .ok_or_else(|| "確認中の操作がありません".to_owned())?;
            if state.action.token != token {
                return Err("この操作確認は無効か、別の会話に属しています".to_owned());
            }
            (
                state.action.clone(),
                state.candidates.clone(),
                state.operation.clone(),
            )
        };
        match operation {
            PendingOperation::ConfirmDelete => {
                if !confirm {
                    return Err("削除には明示的な確認が必要です".to_owned());
                }
                if let Some(key) = candidate_key {
                    if !action
                        .candidates
                        .iter()
                        .any(|candidate| candidate.key == key)
                    {
                        return Err("選択肢が無効です".to_owned());
                    }
                }
                let item = candidates
                    .first()
                    .ok_or_else(|| "削除候補がありません".to_owned())?
                    .clone();
                ensure_fresh(service, &item)?;
                service.delete(&item.id)?;
                pending_map.remove(conversation_id);
                Ok(success(format!("「{}」を削除しました。", item.title), None))
            }
            PendingOperation::Update(_)
            | PendingOperation::Complete
            | PendingOperation::ChooseDelete => {
                if confirm {
                    return Err("この選択操作では確認フラグを使えません".to_owned());
                }
                let key = candidate_key.ok_or_else(|| "候補を選択してください".to_owned())?;
                let index = action
                    .candidates
                    .iter()
                    .position(|candidate| candidate.key == key)
                    .ok_or_else(|| "選択肢が無効です".to_owned())?;
                let item = candidates
                    .get(index)
                    .ok_or_else(|| "選択肢が無効です".to_owned())?
                    .clone();
                ensure_fresh(service, &item)?;
                let outcome = match operation {
                    PendingOperation::Update(input) => update_one(service, &item, input)?,
                    PendingOperation::Complete => complete_one(service, &item)?,
                    PendingOperation::ChooseDelete => {
                        let items = vec![item];
                        let result = self.make_pending(
                            "delete",
                            &format!("「{}」を削除しますか？", items[0].title),
                            &items,
                        );
                        pending_map.remove(conversation_id);
                        store_pending(
                            &mut pending_map,
                            conversation_id,
                            &result,
                            items,
                            PendingOperation::ConfirmDelete,
                        )?;
                        return Ok(result);
                    }
                    PendingOperation::ConfirmDelete => unreachable!(),
                };
                pending_map.remove(conversation_id);
                Ok(outcome)
            }
        }
    }

    pub fn cancel(&self, conversation_id: &str, token: &str) -> Result<(), String> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "Assistantの操作状態を利用できません".to_owned())?;
        let state = pending
            .get(conversation_id)
            .ok_or_else(|| "確認中の操作がありません".to_owned())?;
        if state.action.token != token {
            return Err("この操作確認は無効か、別の会話に属しています".to_owned());
        }
        pending.remove(conversation_id);
        Ok(())
    }

    pub fn pending(&self, conversation_id: &str) -> Result<Option<PendingAction>, String> {
        self.pending
            .lock()
            .map_err(|_| "Assistantの操作状態を利用できません".to_owned())
            .map(|pending| {
                pending
                    .get(conversation_id)
                    .map(|state| state.action.clone())
            })
    }

    pub fn clear(&self, conversation_id: &str) -> Result<(), String> {
        self.pending
            .lock()
            .map_err(|_| "Assistantの操作状態を利用できません".to_owned())?
            .remove(conversation_id);
        Ok(())
    }

    fn make_pending(&self, kind: &str, message: &str, items: &[Item]) -> ToolResult {
        let candidates: Vec<_> = items
            .iter()
            .map(|item| ActionCandidate {
                key: Uuid::new_v4().to_string(),
                title: item.title.clone(),
                kind: item.kind,
                status: item.status,
                notes: item.notes.chars().take(160).collect(),
                project: item.project.clone(),
                scheduled_date: item.scheduled_date.clone(),
                due_date: item.due_date.clone(),
                priority: item.priority,
            })
            .collect();
        let action = PendingAction {
            token: Uuid::new_v4().to_string(),
            kind: kind.to_owned(),
            message: message.to_owned(),
            candidates,
        };
        let candidate_summary: Vec<_> = action.candidates.iter().map(|candidate| json!({
            "title":candidate.title,"kind":candidate.kind,"project":candidate.project,
            "status":candidate.status,"notes":candidate.notes,
            "scheduled_date":candidate.scheduled_date,"due_date":candidate.due_date,"priority":candidate.priority
        })).collect();
        ToolResult {
            output: json!({"message":message,"candidates":candidate_summary}),
            changed: false,
            pending: Some(action),
        }
    }
}

fn store_pending(
    map: &mut HashMap<String, PendingState>,
    conversation_id: &str,
    result: &ToolResult,
    candidates: Vec<Item>,
    operation: PendingOperation,
) -> Result<(), String> {
    let action = result
        .pending
        .clone()
        .ok_or_else(|| "操作確認を作成できませんでした".to_owned())?;
    map.insert(
        conversation_id.to_owned(),
        PendingState {
            action,
            candidates,
            operation,
        },
    );
    Ok(())
}

fn function(name: &str, description: &str, parameters: Value) -> Value {
    json!({"type":"function","function":{"name":name,"description":description,"parameters":parameters}})
}

fn success(message: String, item: Option<Item>) -> ToolResult {
    let mut output = json!({"message":message});
    if let Some(item) = item {
        output["item"] = public_item(&item);
    }
    ToolResult {
        output,
        changed: true,
        pending: None,
    }
}

fn list_items(service: &ItemService, arguments: Value) -> Result<ToolResult, String> {
    let args = object(
        arguments,
        &[
            "kind",
            "status",
            "project",
            "search",
            "due_from",
            "due_to",
            "scheduled_date",
            "due_date",
            "priority",
        ],
    )?;
    let scheduled = optional_date(&args, "scheduled_date")?;
    let exact_due = optional_date(&args, "due_date")?;
    let query = ItemQuery {
        kind: optional_enum(&args, "kind")?,
        status: optional_enum(&args, "status")?,
        project: optional_string(&args, "project")?,
        search: optional_string(&args, "search")?,
        due_from: optional_date(&args, "due_from")?,
        due_to: optional_date(&args, "due_to")?,
    };
    let priority: Option<Priority> = optional_enum(&args, "priority")?;
    let mut items = service.query(&query)?;
    items.retain(|item| {
        scheduled
            .as_ref()
            .is_none_or(|date| item.scheduled_date.as_ref() == Some(date))
            && exact_due
                .as_ref()
                .is_none_or(|date| item.due_date.as_ref() == Some(date))
            && priority.is_none_or(|value| item.priority == value)
    });
    let public: Vec<_> = items.iter().map(public_item).collect();
    Ok(ToolResult {
        output: json!({"message":format!("{}件のアイテムが見つかりました。", public.len()),"items":public}),
        changed: false,
        pending: None,
    })
}

fn create_item(service: &ItemService, arguments: Value) -> Result<ToolResult, String> {
    let args = object(
        arguments,
        &[
            "kind",
            "title",
            "notes",
            "project",
            "scheduled_date",
            "due_date",
            "priority",
            "tags",
        ],
    )?;
    let input = ItemInput {
        kind: required_enum(&args, "kind")?,
        title: required_string(&args, "title")?,
        notes: optional_string(&args, "notes")?.unwrap_or_default(),
        project: optional_string(&args, "project")?,
        scheduled_date: optional_date(&args, "scheduled_date")?,
        due_date: optional_date(&args, "due_date")?,
        priority: optional_enum(&args, "priority")?.unwrap_or(Priority::None),
        tags: optional_strings(&args, "tags")?.unwrap_or_default(),
    };
    let item = service.create(input)?;
    let mut message = format!("「{}」を追加しました。", item.title);
    match (&item.scheduled_date, &item.due_date) {
        (Some(scheduled), Some(due)) => {
            message.push_str(&format!("\n予定日は{scheduled}、締切は{due}です。"))
        }
        (Some(scheduled), None) => message.push_str(&format!("\n予定日は{scheduled}です。")),
        (None, Some(due)) => message.push_str(&format!("\n締切は{due}です。")),
        (None, None) => {}
    }
    Ok(success(message, Some(item)))
}

fn update_arguments(arguments: Value) -> Result<(String, ItemPatch), String> {
    let args = object(
        arguments,
        &[
            "title",
            "new_title",
            "kind",
            "notes",
            "project",
            "scheduled_date",
            "due_date",
            "priority",
            "tags",
        ],
    )?;
    let title = required_string(&args, "title")?;
    if args.len() == 1 {
        return Err("変更する項目を指定してください。".into());
    }
    let patch = ItemPatch {
        kind: optional_enum(&args, "kind")?,
        title: optional_string(&args, "new_title")?,
        notes: optional_string(&args, "notes")?,
        project: nullable_string_patch(&args, "project")?,
        scheduled_date: nullable_date_patch(&args, "scheduled_date")?,
        due_date: nullable_date_patch(&args, "due_date")?,
        priority: optional_enum(&args, "priority")?,
        tags: optional_strings(&args, "tags")?,
    };
    Ok((title, patch))
}

fn update_one(
    service: &ItemService,
    current: &Item,
    patch: ItemPatch,
) -> Result<ToolResult, String> {
    let input = ItemInput {
        kind: patch.kind.unwrap_or(current.kind),
        title: patch.title.unwrap_or_else(|| current.title.clone()),
        notes: patch.notes.unwrap_or_else(|| current.notes.clone()),
        project: patch.project.unwrap_or_else(|| current.project.clone()),
        scheduled_date: patch
            .scheduled_date
            .unwrap_or_else(|| current.scheduled_date.clone()),
        due_date: patch.due_date.unwrap_or_else(|| current.due_date.clone()),
        priority: patch.priority.unwrap_or(current.priority),
        tags: patch.tags.unwrap_or_else(|| current.tags.clone()),
    };
    let item = service.update(&current.id, input)?;
    Ok(success(
        format!("「{}」を更新しました。", item.title),
        Some(item),
    ))
}

fn complete_one(service: &ItemService, current: &Item) -> Result<ToolResult, String> {
    let item = service.complete(&current.id)?;
    Ok(success(
        format!("「{}」を完了にしました。", item.title),
        Some(item),
    ))
}

fn exact_title_matches(service: &ItemService, title: &str) -> Result<Vec<Item>, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("タイトルを指定してください".to_owned());
    }
    Ok(service
        .query(&ItemQuery::default())?
        .into_iter()
        .filter(|item| item.title.eq_ignore_ascii_case(title))
        .collect())
}

fn ensure_fresh(service: &ItemService, snapshot: &Item) -> Result<(), String> {
    let current = service
        .query(&ItemQuery::default())?
        .into_iter()
        .find(|item| item.id == snapshot.id);
    if current
        .as_ref()
        .is_some_and(|item| same_item(item, snapshot))
    {
        Ok(())
    } else {
        Err("対象アイテムが変更または削除されています。最新の一覧から選び直してください".to_owned())
    }
}

fn same_item(a: &Item, b: &Item) -> bool {
    a.id == b.id
        && a.kind == b.kind
        && a.title == b.title
        && a.notes == b.notes
        && a.status == b.status
        && a.project == b.project
        && a.scheduled_date == b.scheduled_date
        && a.due_date == b.due_date
        && a.priority == b.priority
        && a.tags == b.tags
        && a.created_at == b.created_at
        && a.updated_at == b.updated_at
        && a.completed_at == b.completed_at
}

fn public_item(item: &Item) -> Value {
    json!({"kind":item.kind,"title":item.title,"notes":item.notes,"status":item.status,
        "project":item.project,"scheduled_date":item.scheduled_date,"due_date":item.due_date,
        "priority":item.priority,"tags":item.tags})
}

fn object(value: Value, allowed: &[&str]) -> Result<Map<String, Value>, String> {
    let value = value
        .as_object()
        .ok_or_else(|| "操作の引数はオブジェクトで指定してください".to_owned())?;
    if value.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("操作に未対応の項目が含まれています".to_owned());
    }
    Ok(value.clone())
}

fn required_string(args: &Map<String, Value>, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{key}を指定してください"))
}
fn optional_string(args: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(format!("{key}は文字列で指定してください")),
    }
}
fn nullable_string_patch(
    args: &Map<String, Value>,
    key: &str,
) -> Result<Option<Option<String>>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(value)) => Ok(Some(Some(value.clone()))),
        _ => Err(format!("{key}は文字列またはnullで指定してください")),
    }
}
fn required_enum<T: for<'de> Deserialize<'de>>(
    args: &Map<String, Value>,
    key: &str,
) -> Result<T, String> {
    args.get(key)
        .cloned()
        .ok_or_else(|| format!("{key}を指定してください"))
        .and_then(|value| {
            serde_json::from_value(value).map_err(|_| format!("{key}の値が正しくありません"))
        })
}
fn optional_enum<T: for<'de> Deserialize<'de>>(
    args: &Map<String, Value>,
    key: &str,
) -> Result<Option<T>, String> {
    args.get(key)
        .cloned()
        .map(|value| {
            serde_json::from_value(value).map_err(|_| format!("{key}の値が正しくありません"))
        })
        .transpose()
}
fn optional_date(args: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    let value = optional_string(args, key)?;
    validate_date(value, key)
}
fn nullable_date_patch(
    args: &Map<String, Value>,
    key: &str,
) -> Result<Option<Option<String>>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(value)) => validate_date(Some(value.clone()), key).map(Some),
        _ => Err(format!("{key}は日付またはnullで指定してください")),
    }
}
fn validate_date(value: Option<String>, key: &str) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parsed = NaiveDate::parse_from_str(&value, "%Y-%m-%d")
        .map_err(|_| format!("{key}はYYYY-MM-DD形式で指定してください"))?;
    if parsed.format("%Y-%m-%d").to_string() != value {
        return Err(format!("{key}はYYYY-MM-DD形式で指定してください"));
    }
    Ok(Some(value))
}
fn optional_strings(args: &Map<String, Value>, key: &str) -> Result<Option<Vec<String>>, String> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    let values = value
        .as_array()
        .ok_or_else(|| format!("{key}は文字列の配列で指定してください"))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{key}は文字列の配列で指定してください"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn service() -> (tempfile::TempDir, ItemService) {
        let directory = tempdir().unwrap();
        let service = ItemService::open(directory.path().join("items.sqlite")).unwrap();
        (directory, service)
    }

    fn create_args(title: &str) -> Value {
        json!({"kind":"task","title":title,"notes":"keep me","project":"Studio","scheduled_date":"2026-10-08","due_date":"2026-10-12","priority":"high","tags":["ship"]})
    }

    #[test]
    fn policy_executes_clear_requests_without_confirming_or_inventing_attributes() {
        let (_directory, service) = service();
        let tools = AssistantTools::default();
        let today = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let policy = ItemOperationPolicy::new(
            "OSS課題レポートが今日開始の一週間後締め切りでタスク追加してもらえるかな",
            today,
        );
        let added = tools.execute(&service, "c", "create_item", json!({
            "title":"OSS課題レポート","kind":"task",
            "scheduled_date":{"relative":"today"}, "due_date":{"relative":"days_after","days":7},
            "priority":"high","project":"大学","tags":["課題"],"notes":"別の課題から推測"
        }), &policy).unwrap();
        assert!(added.changed && added.pending.is_none());
        let item = service.query(&ItemQuery::default()).unwrap().remove(0);
        assert_eq!(item.scheduled_date.as_deref(), Some("2026-10-07"));
        assert_eq!(item.due_date.as_deref(), Some("2026-10-14"));
        assert_eq!(item.priority, Priority::None);
        assert!(item.project.is_none() && item.tags.is_empty() && item.notes.is_empty());
        assert_eq!(policy.tool_choice()["function"]["name"], "create_item");

        let policy = ItemOperationPolicy::new("OSS課題レポートの締切を10/16にして", today);
        let updated = tools
            .execute(
                &service,
                "c",
                "update_item",
                json!({"title":"OSS課題レポート","due_date":"2026-10-16","scheduled_date":"2026-10-16","priority":"high"}),
                &policy,
            )
            .unwrap();
        assert!(updated.changed && updated.pending.is_none());
        assert_eq!(updated.output["item"]["scheduled_date"], "2026-10-07");
        assert_eq!(updated.output["item"]["due_date"], "2026-10-16");
        assert_eq!(updated.output["item"]["priority"], "none");

        let policy = ItemOperationPolicy::new("OSS課題レポート終わった", today);
        let completed = tools
            .execute(
                &service,
                "c",
                "complete_item",
                json!({"title":"OSS課題レポート"}),
                &policy,
            )
            .unwrap();
        assert!(completed.changed && completed.pending.is_none());
        assert_eq!(completed.output["item"]["status"], "completed");

        let policy = ItemOperationPolicy::new("OSS課題レポートを削除して", today);
        let deletion = tools
            .execute(
                &service,
                "c",
                "delete_item",
                json!({"title":"OSS課題レポート"}),
                &policy,
            )
            .unwrap();
        assert!(!deletion.changed);
        let pending = deletion.pending.unwrap();
        assert_eq!(pending.kind, "delete");
        assert!(tools
            .resolve(&service, "c", &pending.token, None, false)
            .is_err());
        assert_eq!(service.query(&ItemQuery::default()).unwrap().len(), 1);
        tools
            .resolve(&service, "c", &pending.token, None, true)
            .unwrap();
        assert!(service.query(&ItemQuery::default()).unwrap().is_empty());
    }

    #[test]
    fn dispatch_create_patch_preserves_omitted_fields_complete_and_exact_filters() {
        let (_directory, service) = service();
        let tools = AssistantTools::default();
        let created = tools
            .execute_validated(&service, "c1", "create_item", create_args("Release"))
            .unwrap();
        assert!(created.changed);
        let filtered = tools
            .execute_validated(
                &service,
                "c1",
                "list_items",
                json!({"scheduled_date":"2026-10-08","due_date":"2026-10-12","priority":"high"}),
            )
            .unwrap();
        assert_eq!(filtered.output["items"].as_array().unwrap().len(), 1);
        assert!(filtered.output.to_string().find("id").is_none());

        let updated = tools
            .execute_validated(
                &service,
                "c1",
                "update_item",
                json!({"title":"Release","notes":"updated"}),
            )
            .unwrap();
        assert_eq!(updated.output["item"]["notes"], "updated");
        assert_eq!(updated.output["item"]["project"], "Studio");
        assert_eq!(updated.output["item"]["scheduled_date"], "2026-10-08");
        assert_eq!(updated.output["item"]["priority"], "high");
        assert!(tools
            .execute_validated(&service, "c1", "update_item", json!({"title":"Release"}))
            .is_err());
        let cleared = tools
            .execute_validated(
                &service,
                "c1",
                "update_item",
                json!({"title":"Release","scheduled_date":null}),
            )
            .unwrap();
        assert!(cleared.output["item"]["scheduled_date"].is_null());
        assert_eq!(cleared.output["item"]["due_date"], "2026-10-12");
        let completed = tools
            .execute_validated(&service, "c1", "complete_item", json!({"title":"Release"}))
            .unwrap();
        assert_eq!(completed.output["item"]["status"], "completed");
    }

    #[test]
    fn delete_is_scoped_confirmed_once_and_rejects_stale_snapshot() {
        let (_directory, service) = service();
        let tools = AssistantTools::default();
        tools
            .execute_validated(&service, "a", "create_item", create_args("Delete me"))
            .unwrap();
        let pending = tools
            .execute_validated(&service, "a", "delete_item", json!({"title":"Delete me"}))
            .unwrap()
            .pending
            .unwrap();
        assert_eq!(pending.kind, "delete");
        assert!(tools
            .resolve(&service, "b", &pending.token, None, true)
            .is_err());
        assert!(tools
            .resolve(&service, "a", &pending.token, None, false)
            .is_err());
        let current = service.query(&ItemQuery::default()).unwrap().remove(0);
        service
            .update(
                &current.id,
                ItemInput {
                    kind: current.kind,
                    title: current.title,
                    notes: "new version".into(),
                    project: current.project,
                    scheduled_date: current.scheduled_date,
                    due_date: current.due_date,
                    priority: current.priority,
                    tags: current.tags,
                },
            )
            .unwrap();
        assert!(tools
            .resolve(&service, "a", &pending.token, None, true)
            .is_err());
        assert_eq!(service.query(&ItemQuery::default()).unwrap().len(), 1);
        tools.cancel("a", &pending.token).unwrap();

        let next = tools
            .execute_validated(&service, "a", "delete_item", json!({"title":"Delete me"}))
            .unwrap()
            .pending
            .unwrap();
        tools.cancel("a", &next.token).unwrap();
        assert!(tools
            .resolve(&service, "a", &next.token, None, true)
            .is_err());
        let final_confirmation = tools
            .execute_validated(&service, "a", "delete_item", json!({"title":"Delete me"}))
            .unwrap()
            .pending
            .unwrap();
        let deleted = tools
            .resolve(&service, "a", &final_confirmation.token, None, true)
            .unwrap();
        assert!(deleted.changed);
        assert!(tools
            .resolve(&service, "a", &final_confirmation.token, None, true)
            .is_err());
        assert!(service.query(&ItemQuery::default()).unwrap().is_empty());
    }

    #[test]
    fn duplicate_titles_always_require_choice_then_delete_confirmation() {
        let (_directory, service) = service();
        let tools = AssistantTools::default();
        tools
            .execute_validated(&service, "a", "create_item", create_args("Same"))
            .unwrap();
        let mut other = create_args("Same");
        other["kind"] = json!("bute");
        other["project"] = json!("Other");
        tools
            .execute_validated(&service, "a", "create_item", other)
            .unwrap();

        for (name, args) in [
            ("update_item", json!({"title":"Same","notes":"chosen only"})),
            ("complete_item", json!({"title":"Same"})),
        ] {
            let pending = tools
                .execute_validated(&service, "a", name, args)
                .unwrap()
                .pending
                .unwrap();
            assert_eq!(pending.kind, "select");
            let before = service.query(&ItemQuery::default()).unwrap();
            assert!(before
                .iter()
                .all(|item| item.status == ItemStatus::Active && item.notes == "keep me"));
            let selected = tools
                .resolve(
                    &service,
                    "a",
                    &pending.token,
                    Some(&pending.candidates[0].key),
                    false,
                )
                .unwrap();
            assert!(selected.changed);
            let items = service.query(&ItemQuery::default()).unwrap();
            if name == "update_item" {
                assert_eq!(
                    items
                        .iter()
                        .filter(|item| item.notes == "chosen only")
                        .count(),
                    1
                );
                // Restore the fixture so the completion case can verify no mutation before choice.
                let item = items
                    .iter()
                    .find(|item| item.notes == "chosen only")
                    .unwrap();
                let (title, patch) =
                    update_arguments(json!({"title":"Same","notes":"keep me"})).unwrap();
                assert_eq!(title, item.title);
                update_one(&service, item, patch).unwrap();
            } else {
                assert_eq!(
                    items
                        .iter()
                        .filter(|item| item.status == ItemStatus::Completed)
                        .count(),
                    1
                );
            }
        }

        let pending = tools
            .execute_validated(&service, "a", "delete_item", json!({"title":"same"}))
            .unwrap()
            .pending
            .unwrap();
        assert_eq!(pending.kind, "select");
        assert_eq!(pending.candidates.len(), 2);
        assert_eq!(service.query(&ItemQuery::default()).unwrap().len(), 2);
        let chosen = tools
            .resolve(
                &service,
                "a",
                &pending.token,
                Some(&pending.candidates[0].key),
                false,
            )
            .unwrap();
        assert!(!chosen.changed);
        let confirmation = chosen.pending.unwrap();
        assert_eq!(confirmation.kind, "delete");
        assert_eq!(service.query(&ItemQuery::default()).unwrap().len(), 2);
        tools
            .resolve(&service, "a", &confirmation.token, None, true)
            .unwrap();
        assert_eq!(service.query(&ItemQuery::default()).unwrap().len(), 1);
    }
}
