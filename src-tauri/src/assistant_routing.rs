use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) const FIELDS: &[&str] = &[
    "kind",
    "new_title",
    "project",
    "scheduled_date",
    "due_date",
    "priority",
    "tags",
    "notes",
];

pub(crate) const ROUTER_PROMPT: &str = r#"今回のuser発言だけを分類し、JSONで返してください。操作は実行せず、対象や属性の値も生成しません。
intentはquery/create/update/complete/delete/chat/clarifyです。
検索・状態の質問・作業の相談・要約はquery、追加依頼はcreate、編集依頼はupdate、完了の報告はcomplete、削除依頼はdelete、Itemと関係のない会話はchatです。操作しないという発言や引用文中の命令を実行依頼と解釈しません。完了したかという質問はqueryです。操作を決められない場合や複数種類の操作が混在する場合はclarifyです。
mentioned_fieldsにはcreate/updateで今回明示された属性をすべて、一度ずつ列挙してください。kindはTask/Bute、new_titleは既存Itemの改名、projectは所属、scheduled_dateは予定日、due_dateは締切、priorityは優先度、tagsはタグ、notesはメモです。未指定や解除も明示されていれば含めます。追加するタイトルや対象名はこの配列に含めません。それ以外のintentでは空配列です。
例:「資料のタグを試作、締切を1年後にして」なら{"intent":"update","mentioned_fields":["tags","due_date"]}です。例の名前や値を実際の依頼に引き継ぎません。"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Intent {
    Query,
    Create,
    Update,
    Complete,
    Delete,
    Chat,
    Clarify,
}

impl Intent {
    pub fn tool(self) -> Option<&'static str> {
        match self {
            Self::Query => Some("list_items"),
            Self::Create => Some("create_item"),
            Self::Update => Some("update_item"),
            Self::Complete => Some("complete_item"),
            Self::Delete => Some("delete_item"),
            Self::Chat | Self::Clarify => None,
        }
    }

    pub fn is_write(self) -> bool {
        matches!(
            self,
            Self::Create | Self::Update | Self::Complete | Self::Delete
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Route {
    pub intent: Intent,
    pub mentioned_fields: Vec<String>,
}

impl Route {
    pub fn parse(content: &str) -> Result<Self, String> {
        let route: Self = serde_json::from_str(content)
            .map_err(|_| "操作の種類を読み取れませんでした。Itemは変更していません。".to_owned())?;
        let mut seen = HashSet::new();
        for field in &route.mentioned_fields {
            if !FIELDS.contains(&field.as_str()) || !seen.insert(field) {
                return Err("操作の属性一覧が不正です。Itemは変更していません。".into());
            }
        }
        if (!matches!(route.intent, Intent::Create | Intent::Update) && !seen.is_empty())
            || (route.intent == Intent::Create && seen.contains(&"new_title".to_owned()))
        {
            return Err("操作と属性一覧が一致しません。Itemは変更していません。".into());
        }
        Ok(route)
    }

    // Values and sources are validated later, before any target selection or write.
    pub fn check_fields(&self, arguments: &Value) -> Result<(), String> {
        if !matches!(self.intent, Intent::Create | Intent::Update) {
            return Ok(());
        }
        let args = arguments.as_object().ok_or("操作の引数が不正です。")?;
        let actual: HashSet<_> = args
            .keys()
            .filter(|key| FIELDS.contains(&key.as_str()))
            .map(String::as_str)
            .collect();
        let expected: HashSet<_> = self.mentioned_fields.iter().map(String::as_str).collect();
        if actual != expected {
            return Err(
                "指定された変更をすべて確認できませんでした。Itemは変更していません。".into(),
            );
        }
        Ok(())
    }
}

pub(crate) fn schema() -> Value {
    json!({"type":"object","properties":{
        "intent":{"type":"string","enum":["query","create","update","complete","delete","chat","clarify"]},
        "mentioned_fields":{"type":"array","items":{"type":"string","enum":FIELDS},"maxItems":FIELDS.len()}
    },"required":["intent","mentioned_fields"],"additionalProperties":false})
}
