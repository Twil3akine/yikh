use crate::assistant_dates::date_schema;
use crate::assistant_policy::{ItemOperationPolicy, ValidationError};
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
    pub needs_clarification: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAction {
    pub token: String,
    pub kind: String,
    pub operation: String,
    pub message: String,
    pub candidates: Vec<ActionCandidate>,
    pub changes: Vec<ActionChange>,
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
    pub changes: Vec<ActionChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionChange {
    pub label: String,
    pub before: Option<String>,
    pub after: String,
}

struct PendingState {
    action: PendingAction,
    candidates: Vec<Item>,
    operation: PendingOperation,
}

#[derive(Clone)]
enum PendingOperation {
    Create(ItemInput),
    Update(ItemPatch),
    Complete,
    Delete,
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
    pub fn definition(name: &str) -> Result<Value, String> {
        let definition = Self::definitions()
            .as_array()
            .unwrap()
            .iter()
            .find(|definition| definition["function"]["name"] == name)
            .cloned()
            .ok_or("利用できない操作です。")?;
        Ok(json!([definition]))
    }

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
            "create_item",
            "追加内容を確認用に提示します。ユーザーが確認するまで追加しません。任意項目は指定されたものだけ渡してください。",
            optional(p, vec!["title"]),
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
            "指定項目の更新内容を確認用に提示します。同名の場合は対象を選び、内容を確認してから更新します。",
            optional(p, vec!["title"]),
        ));

        let mut p = Map::new();
        p.insert("title".into(), string("完了するアイテムの現在のタイトル"));
        definitions.push(function(
            "complete_item",
            "完了する対象を確認用に提示します。同名の場合は対象を選び、確認してから完了にします。",
            optional(p, vec!["title"]),
        ));

        let mut p = Map::new();
        p.insert("title".into(), string("削除するアイテムの現在のタイトル"));
        definitions.push(function(
            "delete_item",
            "削除候補を表示し、ユーザー確認後に削除します。",
            optional(p, vec!["title"]),
        ));

        for definition in &mut definitions {
            if definition["function"]["name"] == "list_items" {
                continue;
            }
            let properties = definition["function"]["parameters"]["properties"]
                .as_object_mut()
                .unwrap();
            if let Some(title) = properties.remove("title") {
                let variants: Vec<_> = std::mem::take(properties)
                    .into_iter()
                    .map(|(field, schema)| {
                        json!({"type":"object","properties":{
                        "field":{"type":"string","enum":[field]},
                        "value":schema,
                        "source":string("今回の発言でこの項目を指定した箇所をそのまま引用します")
                    },"required":["field","value","source"],"additionalProperties":false})
                    })
                    .collect();
                properties.insert("title".into(), title);
                if !variants.is_empty() {
                    properties.insert("changes".into(), json!({
                        "type":"array","description":"今回指定された属性をすべて、一度ずつ列挙します。未指定属性は追加しません",
                        "items":{"oneOf":variants},"maxItems":variants.len()
                    }));
                    definition["function"]["parameters"]["required"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!("changes"));
                }
            }
            let properties = definition["function"]["parameters"]["properties"]
                .as_object_mut()
                .unwrap();
            properties.insert(
                "reference".into(),
                string("今回の依頼原文と対象タイトルの両方に含まれる名前の部分を原文のまま引用します。正式タイトルへ補完しません"),
            );
            definition["function"]["parameters"]["required"]
                .as_array_mut()
                .unwrap()
                .push(json!("reference"));
        }

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
        let validated = match policy.validate(name, arguments) {
            Ok(validated) => validated,
            Err(ValidationError::Clarification(question)) => return Ok(clarification(question)),
            Err(ValidationError::Invalid(error)) => return Err(error),
        };
        let arguments = validated.arguments;
        let targets = if matches!(name, "update_item" | "complete_item" | "delete_item") {
            let items = service.query(&ItemQuery::default())?;
            if items.is_empty() {
                return Ok(clarification(
                    "対象のアイテムが見つかりません。操作したいアイテムのタイトルを教えてください。".into(),
                ));
            }
            Some(crate::assistant_targets::resolve_targets(
                items,
                validated.reference.as_deref().unwrap_or_default(),
                arguments["title"]
                    .as_str()
                    .ok_or("対象のタイトルを指定してください。")?,
            )?)
        } else {
            None
        };
        self.dispatch(service, conversation_id, name, arguments, targets)
    }

    #[cfg(test)]
    fn execute_validated(
        &self,
        service: &ItemService,
        conversation_id: &str,
        name: &str,
        arguments: Value,
    ) -> Result<ToolResult, String> {
        self.dispatch(service, conversation_id, name, arguments, None)
    }

    fn dispatch(
        &self,
        service: &ItemService,
        conversation_id: &str,
        name: &str,
        arguments: Value,
        targets: Option<crate::assistant_targets::TargetResolution>,
    ) -> Result<ToolResult, String> {
        let needs_selection = targets
            .as_ref()
            .is_some_and(|targets| targets.needs_confirmation);
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "Assistantの操作状態を利用できません".to_owned())?;
        if pending.contains_key(conversation_id) {
            return Err("先に表示中の操作を選択、確認、またはキャンセルしてください".to_owned());
        }
        let (matches, operation) = match name {
            "list_items" => return list_items(service, arguments),
            "create_item" => (
                Vec::new(),
                PendingOperation::Create(create_arguments(arguments)?),
            ),
            "update_item" => {
                let (title, patch) = update_arguments(arguments)?;
                let matches = match targets {
                    Some(targets) => targets.items,
                    None => exact_title_matches(service, &title)?,
                };
                (matches, PendingOperation::Update(patch))
            }
            "complete_item" | "delete_item" => {
                let args = object(arguments, &["title"])?;
                let title = required_string(&args, "title")?;
                let matches = match targets {
                    Some(targets) => targets.items,
                    None => exact_title_matches(service, &title)?,
                };
                let operation = if name == "complete_item" {
                    PendingOperation::Complete
                } else {
                    PendingOperation::Delete
                };
                (matches, operation)
            }
            _ => return Err("利用できない操作です".to_owned()),
        };
        let result = if matches!(operation, PendingOperation::Create(_)) {
            self.make_confirmation(name, &operation, &matches)?
        } else if matches.is_empty() {
            return Err("該当するアイテムが見つかりません".to_owned());
        } else if matches.len() == 1 && !needs_selection {
            self.make_confirmation(name, &operation, &matches)?
        } else {
            self.make_pending(
                "select",
                name,
                "対象のアイテムを選んでください。選択後に操作内容を確認します。",
                &operation,
                &matches,
            )
        };
        store_pending(&mut pending, conversation_id, &result, matches, operation)?;
        Ok(result)
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
        if action.kind == "select" {
            if confirm {
                return Err("対象を選んでから操作内容を確認してください".to_owned());
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
            let items = vec![item];
            let result = self.make_confirmation(&action.operation, &operation, &items)?;
            store_pending(&mut pending_map, conversation_id, &result, items, operation)?;
            return Ok(result);
        }
        if !confirm {
            return Err("実行には明示的な確認が必要です".to_owned());
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
        let result = match operation {
            PendingOperation::Create(input) => create_one(service, input)?,
            operation => {
                let item = candidates
                    .first()
                    .ok_or_else(|| "対象がありません".to_owned())?;
                ensure_fresh(service, item)?;
                match operation {
                    PendingOperation::Update(patch) => update_one(service, item, patch)?,
                    PendingOperation::Complete => complete_one(service, item)?,
                    PendingOperation::Delete => {
                        service.delete(&item.id)?;
                        success(format!("「{}」を削除しました。", item.title), None)
                    }
                    PendingOperation::Create(_) => unreachable!(),
                }
            }
        };
        pending_map.remove(conversation_id);
        Ok(result)
    }

    fn make_confirmation(
        &self,
        name: &str,
        operation: &PendingOperation,
        items: &[Item],
    ) -> Result<ToolResult, String> {
        let message = match operation {
            PendingOperation::Create(input) => format!(
                "次の内容で追加してよいですか？\n{}",
                patch_description(&creation_patch(input))
            ),
            operation => {
                let item = items.first().ok_or_else(|| "対象がありません".to_owned())?;
                match operation {
                    PendingOperation::Update(patch) => format!(
                        "「{}」を次の内容で更新してよいですか？\n{}",
                        item.title,
                        patch_description(patch)
                    ),
                    PendingOperation::Complete => {
                        format!("「{}」を完了にしてよいですか？", item.title)
                    }
                    PendingOperation::Delete => format!("「{}」を削除しますか？", item.title),
                    PendingOperation::Create(_) => unreachable!(),
                }
            }
        };
        let kind = if matches!(operation, PendingOperation::Delete) {
            "delete"
        } else {
            "confirm"
        };
        Ok(self.make_pending(kind, name, &message, operation, items))
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

    fn make_pending(
        &self,
        kind: &str,
        name: &str,
        message: &str,
        operation: &PendingOperation,
        items: &[Item],
    ) -> ToolResult {
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
                changes: operation_changes(operation, Some(item)),
            })
            .collect();
        let action = PendingAction {
            token: Uuid::new_v4().to_string(),
            kind: kind.to_owned(),
            operation: name.to_owned(),
            message: message.to_owned(),
            candidates,
            changes: if matches!(operation, PendingOperation::Create(_)) {
                operation_changes(operation, None)
            } else {
                Vec::new()
            },
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
            needs_clarification: false,
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
        needs_clarification: false,
    }
}

fn clarification(message: String) -> ToolResult {
    ToolResult {
        output: json!({"message":message}),
        changed: false,
        pending: None,
        needs_clarification: true,
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
        needs_clarification: false,
    })
}

fn create_arguments(arguments: Value) -> Result<ItemInput, String> {
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
    Ok(input)
}

fn create_one(service: &ItemService, input: ItemInput) -> Result<ToolResult, String> {
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

fn creation_patch(input: &ItemInput) -> ItemPatch {
    ItemPatch {
        title: Some(input.title.clone()),
        kind: Some(input.kind),
        notes: Some(input.notes.clone()),
        project: Some(input.project.clone()),
        scheduled_date: Some(input.scheduled_date.clone()),
        due_date: Some(input.due_date.clone()),
        priority: Some(input.priority),
        tags: Some(input.tags.clone()),
    }
}

fn operation_changes(operation: &PendingOperation, item: Option<&Item>) -> Vec<ActionChange> {
    match operation {
        PendingOperation::Create(input) => patch_changes(&creation_patch(input), None),
        PendingOperation::Update(patch) => patch_changes(patch, item),
        PendingOperation::Complete => vec![ActionChange {
            label: "状態".into(),
            before: item.map(|item| match item.status {
                ItemStatus::Active => "未完了".into(),
                ItemStatus::Completed => "完了".into(),
            }),
            after: "完了".into(),
        }],
        PendingOperation::Delete => vec![ActionChange {
            label: "操作".into(),
            before: None,
            after: "このアイテムを削除".into(),
        }],
    }
}

fn patch_changes(patch: &ItemPatch, current: Option<&Item>) -> Vec<ActionChange> {
    let text = |value: &str| {
        if value.trim().is_empty() {
            "未設定".into()
        } else {
            value.to_owned()
        }
    };
    let optional = |value: &Option<String>| text(value.as_deref().unwrap_or_default());
    let kind = |kind| match kind {
        ItemKind::Task => "Task".into(),
        ItemKind::Bute => "Bute".into(),
    };
    let priority = |priority| match priority {
        Priority::None => "未設定".into(),
        Priority::Low => "低".into(),
        Priority::Medium => "中".into(),
        Priority::High => "高".into(),
    };
    let tags = |tags: &[String]| text(&tags.join("、"));
    [
        (
            "タイトル",
            current.map(|item| item.title.clone()),
            patch.title.clone(),
        ),
        (
            "種類",
            current.map(|item| kind(item.kind)),
            patch.kind.map(kind),
        ),
        (
            "プロジェクト",
            current.map(|item| optional(&item.project)),
            patch.project.as_ref().map(optional),
        ),
        (
            "予定日",
            current.map(|item| optional(&item.scheduled_date)),
            patch.scheduled_date.as_ref().map(optional),
        ),
        (
            "締切日",
            current.map(|item| optional(&item.due_date)),
            patch.due_date.as_ref().map(optional),
        ),
        (
            "優先度",
            current.map(|item| priority(item.priority)),
            patch.priority.map(priority),
        ),
        (
            "タグ",
            current.map(|item| tags(&item.tags)),
            patch.tags.as_ref().map(|value| tags(value)),
        ),
        (
            "メモ",
            current.map(|item| text(&item.notes)),
            patch.notes.as_ref().map(|value| text(value)),
        ),
    ]
    .into_iter()
    .filter_map(|(label, before, after)| {
        after.map(|after| ActionChange {
            label: label.into(),
            before,
            after,
        })
    })
    .collect()
}

fn patch_description(patch: &ItemPatch) -> String {
    patch_changes(patch, None)
        .into_iter()
        .map(|change| format!("{}: {}", change.label, change.after))
        .collect::<Vec<_>>()
        .join("\n")
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

    fn approve(
        tools: &AssistantTools,
        service: &ItemService,
        id: &str,
        proposal: ToolResult,
    ) -> ToolResult {
        assert!(!proposal.changed);
        let pending = proposal.pending.unwrap();
        assert!(matches!(pending.kind.as_str(), "confirm" | "delete"));
        assert!(tools
            .resolve(service, id, &pending.token, None, false)
            .is_err());
        assert!(tools
            .resolve(service, "other-conversation", &pending.token, None, true)
            .is_err());
        let result = tools
            .resolve(service, id, &pending.token, None, true)
            .unwrap();
        assert!(result.changed && result.pending.is_none());
        assert!(tools
            .resolve(service, id, &pending.token, None, true)
            .is_err());
        result
    }

    fn with_sources(request: &str, reference: &str, mut args: Value, fields: &[&str]) -> Value {
        let object = args.as_object_mut().unwrap();
        let changes: Vec<_> = fields
            .iter()
            .map(|field| {
                let value = object.remove(*field).unwrap();
                json!({"field":field,"value":value,"source":request})
            })
            .collect();
        if !fields.is_empty() {
            object.insert("changes".into(), json!(changes));
        }
        object.insert("reference".into(), json!(reference));
        args
    }

    #[test]
    fn unquoted_targets_require_selection_and_deletion_still_requires_confirmation() {
        for tool in ["complete_item", "delete_item"] {
            let (_directory, service) = service();
            let tools = AssistantTools::default();
            for title in ["Aufyの開発", "Aufyの検証"] {
                create_one(&service, create_arguments(create_args(title)).unwrap()).unwrap();
            }
            let policy = ItemOperationPolicy::new(
                "Aufyを操作して",
                NaiveDate::from_ymd_opt(2026, 10, 7).unwrap(),
            );
            let result = tools
                .execute(
                    &service,
                    "c",
                    tool,
                    json!({"title":"Aufy","reference":"Aufyの開発"}),
                    &policy,
                )
                .unwrap();
            assert!(!result.changed);
            let pending = result.pending.unwrap();
            assert_eq!(pending.kind, "select");
            assert_eq!(pending.operation, tool);
            assert_eq!(pending.candidates.len(), 2);
            assert!(service
                .query(&ItemQuery::default())
                .unwrap()
                .iter()
                .all(|item| item.status == ItemStatus::Active));
            let selected = &pending.candidates[0];
            let result = tools
                .resolve(&service, "c", &pending.token, Some(&selected.key), false)
                .unwrap();
            if tool == "delete_item" {
                assert!(!result.changed);
                let confirmation = result.pending.unwrap();
                assert_eq!(confirmation.kind, "delete");
                assert_eq!(confirmation.operation, "delete_item");
                assert_eq!(service.query(&ItemQuery::default()).unwrap().len(), 2);
                assert!(tools
                    .resolve(&service, "c", &confirmation.token, None, false)
                    .is_err());
                assert!(
                    tools
                        .resolve(&service, "c", &confirmation.token, None, true)
                        .unwrap()
                        .changed
                );
                let remaining = service.query(&ItemQuery::default()).unwrap();
                assert_eq!(remaining.len(), 1);
                assert_ne!(remaining[0].title, selected.title);
            } else {
                assert!(!result.changed);
                assert!(service
                    .query(&ItemQuery::default())
                    .unwrap()
                    .iter()
                    .all(|item| item.status == ItemStatus::Active));
                let result = approve(&tools, &service, "c", result);
                assert!(result.changed && result.pending.is_none());
                let items = service.query(&ItemQuery::default()).unwrap();
                assert_eq!(
                    items
                        .iter()
                        .filter(|item| item.status == ItemStatus::Completed)
                        .count(),
                    1
                );
                assert_eq!(
                    items
                        .iter()
                        .find(|item| item.title == selected.title)
                        .unwrap()
                        .status,
                    ItemStatus::Completed
                );
            }
        }
    }

    #[test]
    fn all_writes_require_confirmation_without_inventing_attributes() {
        let (_directory, service) = service();
        let tools = AssistantTools::default();
        let today = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let policy = ItemOperationPolicy::new(
            "OSS課題レポートが今日開始の一週間後締め切りでタスク追加してもらえるかな",
            today,
        );
        let added = tools.execute(&service, "c", "create_item", with_sources(
            policy.request(), "OSS課題レポート", json!({
            "title":"OSS課題レポート","kind":"task",
            "scheduled_date":{"relative":"today"}, "due_date":{"relative":"days_after","days":7}
            }), &["kind", "scheduled_date", "due_date"]), &policy).unwrap();
        assert!(!added.changed);
        assert_eq!(added.pending.as_ref().unwrap().operation, "create_item");
        assert!(added.pending.as_ref().unwrap().candidates.is_empty());
        let added = approve(&tools, &service, "c", added);
        assert!(added.changed && added.pending.is_none());
        let item = service.query(&ItemQuery::default()).unwrap().remove(0);
        assert_eq!(item.scheduled_date.as_deref(), Some("2026-10-07"));
        assert_eq!(item.due_date.as_deref(), Some("2026-10-14"));
        assert_eq!(item.priority, Priority::None);
        assert!(item.project.is_none() && item.tags.is_empty() && item.notes.is_empty());
        let policy = ItemOperationPolicy::new("OSS課題レポートの締切を10/16にして", today);
        let updated = tools
            .execute(
                &service,
                "c",
                "update_item",
                with_sources(
                    policy.request(),
                    "OSS課題レポート",
                    json!({"title":"OSS課題レポート","due_date":"2026-10-16"}),
                    &["due_date"],
                ),
                &policy,
            )
            .unwrap();
        assert_eq!(
            service.query(&ItemQuery::default()).unwrap()[0]
                .due_date
                .as_deref(),
            Some("2026-10-14")
        );
        let updated = approve(&tools, &service, "c", updated);
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
                with_sources(
                    policy.request(),
                    "OSS課題レポート",
                    json!({"title":"OSS課題レポート"}),
                    &[],
                ),
                &policy,
            )
            .unwrap();
        assert_eq!(
            service.query(&ItemQuery::default()).unwrap()[0].status,
            ItemStatus::Active
        );
        let completed = approve(&tools, &service, "c", completed);
        assert!(completed.changed && completed.pending.is_none());
        assert_eq!(completed.output["item"]["status"], "completed");

        let policy = ItemOperationPolicy::new("OSS課題レポートを削除して", today);
        let deletion = tools
            .execute(
                &service,
                "c",
                "delete_item",
                with_sources(
                    policy.request(),
                    "OSS課題レポート",
                    json!({"title":"OSS課題レポート"}),
                    &[],
                ),
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
    fn reported_completion_and_bute_creation_persist_the_actual_results() {
        let (directory, service) = service();
        let tools = AssistantTools::default();
        let today = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let create_policy = ItemOperationPolicy::new(
            "OSS課題レポートが今日開始の一週間後締め切りでタスク追加してもらえるかな",
            today,
        );
        let proposal = tools.execute(&service, "c", "create_item", with_sources(create_policy.request(), "OSS課題レポート", json!({"title":"OSS課題レポート","kind":"task","scheduled_date":{"relative":"today"},"due_date":{"relative":"days_after","days":7}}), &["kind", "scheduled_date", "due_date"]), &create_policy).unwrap();
        approve(&tools, &service, "c", proposal);
        let completion_policy = ItemOperationPolicy::new("OSSのレポートできた", today);
        let completion = tools
            .execute(
                &service,
                "c",
                "complete_item",
                with_sources(
                    completion_policy.request(),
                    "OSS",
                    json!({"title":"OSS課題レポート"}),
                    &[],
                ),
                &completion_policy,
            )
            .unwrap();
        assert_eq!(
            service.query(&ItemQuery::default()).unwrap()[0].status,
            ItemStatus::Active
        );
        let completion = approve(&tools, &service, "c", completion);
        assert!(completion.changed && completion.pending.is_none());
        let completed = service.query(&ItemQuery::default()).unwrap().remove(0);
        assert_eq!(completed.status, ItemStatus::Completed);
        assert!(completed.completed_at.is_some());

        let bute_policy =
            ItemOperationPolicy::new("butesにAufyの開発を無期限で入れといて、優先度低めで", today);
        let added = tools
            .execute(
                &service,
                "c",
                "create_item",
                with_sources(
                    bute_policy.request(),
                    "Aufyの開発",
                    json!({"title":"Aufyの開発","kind":"bute","due_date":null,"priority":"low"}),
                    &["kind", "due_date", "priority"],
                ),
                &bute_policy,
            )
            .unwrap();
        assert!(!added.changed);
        assert_eq!(added.pending.as_ref().unwrap().operation, "create_item");
        assert!(added.pending.as_ref().unwrap().candidates.is_empty());
        let added = approve(&tools, &service, "c", added);
        assert!(added.changed && added.pending.is_none());
        assert!(!added.output["message"]
            .as_str()
            .unwrap()
            .contains("プロジェクト"));
        drop(service);
        let reopened = ItemService::open(directory.path().join("items.sqlite")).unwrap();
        let items = reopened.query(&ItemQuery::default()).unwrap();
        let item = items
            .iter()
            .find(|item| item.title == "Aufyの開発")
            .unwrap();
        assert_eq!(item.kind, ItemKind::Bute);
        assert_eq!(item.priority, Priority::Low);
        assert!(
            item.due_date.is_none()
                && item.project.is_none()
                && item.tags.is_empty()
                && item.notes.is_empty()
        );
        let completed = items
            .iter()
            .find(|item| item.title == "OSS課題レポート")
            .unwrap();
        assert_eq!(completed.status, ItemStatus::Completed);

        // A model-proposed full title must not bypass ambiguity in the user's alias.
        create_one(
            &reopened,
            create_arguments(create_args("OSS研究レポート")).unwrap(),
        )
        .unwrap();
        let pending = tools
            .execute(
                &reopened,
                "c",
                "complete_item",
                with_sources(
                    completion_policy.request(),
                    "OSS",
                    json!({"title":"OSS課題レポート"}),
                    &[],
                ),
                &completion_policy,
            )
            .unwrap();
        assert!(!pending.changed);
        assert_eq!(pending.pending.unwrap().candidates.len(), 2);
        assert_eq!(
            reopened
                .query(&ItemQuery {
                    status: Some(ItemStatus::Active),
                    ..ItemQuery::default()
                })
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn dispatch_create_patch_preserves_omitted_fields_complete_and_exact_filters() {
        let (_directory, service) = service();
        let tools = AssistantTools::default();
        let proposal = tools
            .execute_validated(&service, "c1", "create_item", create_args("Release"))
            .unwrap();
        assert!(service.query(&ItemQuery::default()).unwrap().is_empty());
        let created = approve(&tools, &service, "c1", proposal);
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
        assert_eq!(
            service.query(&ItemQuery::default()).unwrap()[0].notes,
            "keep me"
        );
        let updated = approve(&tools, &service, "c1", updated);
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
        let cleared = approve(&tools, &service, "c1", cleared);
        assert!(cleared.output["item"]["scheduled_date"].is_null());
        assert_eq!(cleared.output["item"]["due_date"], "2026-10-12");
        let completed = tools
            .execute_validated(&service, "c1", "complete_item", json!({"title":"Release"}))
            .unwrap();
        let completed = approve(&tools, &service, "c1", completed);
        assert_eq!(completed.output["item"]["status"], "completed");
    }

    #[test]
    fn previews_show_each_candidates_actual_values_and_all_planned_changes() {
        let (_directory, service) = service();
        let tools = AssistantTools::default();
        let proposal = tools
            .execute_validated(&service, "c", "create_item", create_args("Same"))
            .unwrap();
        let preview = serde_json::to_value(&proposal.pending.as_ref().unwrap().changes).unwrap();
        assert!(proposal.pending.as_ref().unwrap().candidates.is_empty());
        assert_eq!(preview.as_array().unwrap().len(), 8);
        assert!(preview
            .as_array()
            .unwrap()
            .iter()
            .all(|change| change["before"].is_null()));
        assert_eq!(preview[0]["after"], "Same");
        assert_eq!(preview[4]["after"], "2026-10-12");
        assert!(service.query(&ItemQuery::default()).unwrap().is_empty());
        approve(&tools, &service, "c", proposal);
        let mut other = create_args("Same");
        other["project"] = json!("Other");
        other["tags"] = json!([]);
        create_one(&service, create_arguments(other).unwrap()).unwrap();
        let before = service.query(&ItemQuery::default()).unwrap();
        let notes = format!("**メモ**\n{}", "省略しない内容".repeat(40));
        let proposal = tools
            .execute_validated(
                &service,
                "c",
                "update_item",
                json!({
                    "title":"Same", "tags":["gwitg"], "due_date":null, "notes":notes
                }),
            )
            .unwrap();
        let pending = proposal.pending.unwrap();
        assert_eq!(pending.kind, "select");
        assert!(pending.changes.is_empty());
        assert_eq!(pending.candidates.len(), 2);
        for candidate in &pending.candidates {
            let preview = serde_json::to_value(&candidate.changes).unwrap();
            assert_eq!(
                preview,
                json!([
                    {"label":"締切日", "before":"2026-10-12", "after":"未設定"},
                    {"label":"タグ", "before":if candidate.project.as_deref() == Some("Other") { "未設定" } else { "ship" }, "after":"gwitg"},
                    {"label":"メモ", "before":"keep me", "after":notes}
                ])
            );
        }
        let selected = &pending.candidates[0];
        let result = tools
            .resolve(&service, "c", &pending.token, Some(&selected.key), false)
            .unwrap();
        let confirmation = result.pending.as_ref().unwrap();
        assert_eq!(confirmation.kind, "confirm");
        assert_eq!(
            serde_json::to_value(&confirmation.candidates[0].changes).unwrap(),
            serde_json::to_value(&selected.changes).unwrap()
        );
        assert!(!result.changed);
        assert_eq!(
            serde_json::to_value(service.query(&ItemQuery::default()).unwrap()).unwrap(),
            serde_json::to_value(before).unwrap()
        );
        let updated = approve(&tools, &service, "c", result);
        assert_eq!(updated.output["item"]["tags"], json!(["gwitg"]));
        assert!(updated.output["item"]["due_date"].is_null());
        assert_eq!(updated.output["item"]["notes"], notes);
        for (name, label, expected_before, after) in [
            ("complete_item", "状態", Some("未完了"), "完了"),
            ("delete_item", "操作", None, "このアイテムを削除"),
        ] {
            let pending = tools
                .execute_validated(&service, "c", name, json!({"title":"Same"}))
                .unwrap()
                .pending
                .unwrap();
            for candidate in &pending.candidates {
                assert_eq!(
                    serde_json::to_value(&candidate.changes).unwrap(),
                    json!([
                        {"label":label, "before":expected_before, "after":after}
                    ])
                );
            }
            tools.cancel("c", &pending.token).unwrap();
        }
        assert_eq!(service.query(&ItemQuery::default()).unwrap().len(), 2);
    }

    #[test]
    fn cancelled_or_cleared_confirmations_never_execute_any_write() {
        for (name, arguments) in [
            ("create_item", create_args("New")),
            ("update_item", json!({"title":"Existing","notes":"changed"})),
            ("complete_item", json!({"title":"Existing"})),
            ("delete_item", json!({"title":"Existing"})),
        ] {
            let (_directory, service) = service();
            let tools = AssistantTools::default();
            create_one(&service, create_arguments(create_args("Existing")).unwrap()).unwrap();
            let before =
                serde_json::to_value(service.query(&ItemQuery::default()).unwrap()).unwrap();
            for clear in [false, true] {
                let proposal = tools
                    .execute_validated(&service, "c", name, arguments.clone())
                    .unwrap();
                let pending = proposal.pending.unwrap();
                assert!(!proposal.changed);
                assert!(tools
                    .resolve(&service, "c", &pending.token, None, false)
                    .is_err());
                if clear {
                    tools.clear("c").unwrap();
                } else {
                    tools.cancel("c", &pending.token).unwrap();
                }
                assert!(tools
                    .resolve(&service, "c", &pending.token, None, true)
                    .is_err());
                assert_eq!(
                    serde_json::to_value(service.query(&ItemQuery::default()).unwrap()).unwrap(),
                    before
                );
            }
        }
    }

    #[test]
    fn confirmations_are_scoped_single_use_and_reject_stale_snapshots() {
        let (_directory, service) = service();
        let tools = AssistantTools::default();
        create_one(
            &service,
            create_arguments(create_args("Delete me")).unwrap(),
        )
        .unwrap();
        for (name, args) in [
            (
                "update_item",
                json!({"title":"Delete me","notes":"assistant value"}),
            ),
            ("complete_item", json!({"title":"Delete me"})),
        ] {
            let pending = tools
                .execute_validated(&service, "a", name, args)
                .unwrap()
                .pending
                .unwrap();
            let item = service.query(&ItemQuery::default()).unwrap().remove(0);
            let (_, patch) = update_arguments(
                json!({"title":"Delete me","notes":format!("GUI change: {name}")}),
            )
            .unwrap();
            update_one(&service, &item, patch).unwrap();
            assert!(tools
                .resolve(&service, "a", &pending.token, None, true)
                .is_err());
            let unchanged = service.query(&ItemQuery::default()).unwrap().remove(0);
            assert_eq!(unchanged.notes, format!("GUI change: {name}"));
            assert_eq!(unchanged.status, ItemStatus::Active);
            tools.cancel("a", &pending.token).unwrap();
        }
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
        create_one(&service, create_arguments(create_args("Same")).unwrap()).unwrap();
        let mut other = create_args("Same");
        other["kind"] = json!("bute");
        other["project"] = json!("Other");
        create_one(&service, create_arguments(other).unwrap()).unwrap();

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
            assert!(!selected.changed);
            assert!(tools
                .resolve(&service, "a", &pending.token, None, true)
                .is_err());
            let unchanged = service.query(&ItemQuery::default()).unwrap();
            assert!(unchanged
                .iter()
                .all(|item| item.status == ItemStatus::Active && item.notes == "keep me"));
            let selected = approve(&tools, &service, "a", selected);
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
