use crate::assistant_dates::resolve_date;
use crate::assistant_routing::{ItemPlan, FIELDS as MUTABLE_FIELDS};
use crate::model::Item;
use chrono::NaiveDate;
use serde_json::{json, Value};

/// The model interprets the request; Rust validates its structured arguments.
/// Evidence must come from this request, never from Item data or conversation history.
pub(crate) struct ItemOperationPolicy {
    request: String,
    today: NaiveDate,
    follow_up_target: Option<Item>,
}

#[derive(Debug)]
pub(crate) struct ValidatedOperation {
    pub arguments: Value,
    /// None means the model's target reference needs user confirmation.
    pub reference: Option<String>,
}

#[derive(Debug)]
pub(crate) enum ValidationError {
    Invalid(String),
    Clarification(String),
}

impl From<String> for ValidationError {
    fn from(message: String) -> Self {
        Self::Invalid(message)
    }
}

impl From<&str> for ValidationError {
    fn from(message: &str) -> Self {
        Self::Invalid(message.into())
    }
}

impl ItemOperationPolicy {
    pub fn new(request: &str, today: NaiveDate) -> Self {
        Self {
            request: request.to_lowercase(),
            today,
            follow_up_target: None,
        }
    }

    pub(crate) fn with_follow_up_target(mut self, target: Option<Item>) -> Self {
        self.follow_up_target = target;
        self
    }

    pub(crate) fn follow_up_target(&self) -> Option<&Item> {
        self.follow_up_target.as_ref()
    }

    #[cfg(test)]
    pub fn request(&self) -> &str {
        &self.request
    }

    fn contains_source(&self, source: &str) -> bool {
        !source.trim().is_empty() && self.request.contains(&source.to_lowercase())
    }

    pub fn validate(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<ValidatedOperation, ValidationError> {
        if !matches!(
            name,
            "list_items" | "create_item" | "update_item" | "complete_item" | "delete_item"
        ) {
            return Err("利用できない操作です。".into());
        }
        let arguments = if matches!(name, "create_item" | "update_item") {
            let plan = ItemPlan::parse(arguments)?;
            let mut args = serde_json::Map::new();
            args.insert("title".into(), json!(plan.title));
            args.insert("reference".into(), json!(plan.reference));
            for change in plan.changes {
                args.insert(
                    change.field,
                    json!({"value":change.value,"source":change.source}),
                );
            }
            Value::Object(args)
        } else {
            arguments
        };
        let mut args = arguments
            .as_object()
            .cloned()
            .ok_or("Toolの引数はオブジェクトで指定してください。")?;
        let mut reference = None;
        if name != "list_items" {
            // Target evidence and attribute evidence are independent. A target
            // quote mismatch may require selection, but never authorizes a write.
            let raw_reference = args.remove("reference");
            let omitted_reference = raw_reference.as_ref().and_then(Value::as_str) == Some("");
            reference = raw_reference
                .and_then(|value| value.as_str().map(str::to_owned))
                .filter(|value| self.contains_source(value));
            // Only an explicitly omitted target may use the last confirmed Item
            // as a suggestion. Attribute evidence still comes from this request.
            if omitted_reference && name != "create_item" {
                if let Some(target) = self.follow_up_target() {
                    args.insert("title".into(), json!(target.title));
                }
            }
            // Each supplied field carries its own value and evidence. Reject
            // incomplete arguments instead of silently dropping requested changes.
            for field in MUTABLE_FIELDS {
                let Some(attribute) = args.get(*field) else {
                    continue;
                };
                let attribute = attribute
                    .as_object()
                    .filter(|attribute| {
                        attribute.len() == 2
                            && attribute.contains_key("value")
                            && attribute.contains_key("source")
                    })
                    .ok_or_else(|| {
                        format!("Tool引数の{field}にvalueとsourceを一組で渡してください。")
                    })?;
                if !attribute["source"]
                    .as_str()
                    .is_some_and(|source| self.contains_source(source))
                {
                    let label = field_label(field);
                    return Err(ValidationError::Clarification(format!(
                        "{label}の変更内容を確認できませんでした。{label}をどう設定するか教えてください。"
                    )));
                }
                args.insert((*field).into(), attribute["value"].clone());
            }
            let title = args.get("title").and_then(Value::as_str).ok_or_else(|| {
                ValidationError::Clarification(
                    "どのアイテムを操作しますか？タイトルを教えてください。".into(),
                )
            })?;
            if title.trim().is_empty() || (name == "create_item" && !self.contains_source(title)) {
                return Err(ValidationError::Clarification(
                    "操作するアイテムのタイトルを教えてください。".into(),
                ));
            }
            if name == "create_item" && reference.is_none() {
                return Err(ValidationError::Clarification(
                    "追加するアイテムのタイトルを教えてください。".into(),
                ));
            }
            if name == "create_item" && !args.contains_key("kind") {
                args.insert("kind".into(), json!("task"));
            }
            if name == "update_item" && args.len() == 1 {
                return Err(ValidationError::Clarification(
                    "どの項目を、どの値に変更しますか？".into(),
                ));
            }
        }
        for field in ["scheduled_date", "due_date", "due_from", "due_to"] {
            if name == "create_item" && args.get(field).is_some_and(Value::is_null) {
                args.remove(field);
                continue;
            }
            if let Some(value) = args.get(field) {
                let date = resolve_date(self.today, value).map_err(|error| {
                    format!("{}の値を確認してください。{error}", field_label(field))
                })?;
                args.insert(field.into(), date);
            }
        }
        Ok(ValidatedOperation {
            arguments: Value::Object(args),
            reference,
        })
    }
}

fn field_label(field: &str) -> &str {
    match field {
        "kind" => "種類",
        "new_title" => "タイトル",
        "priority" => "優先度",
        "project" => "プロジェクト",
        "tags" => "タグ",
        "notes" => "メモ",
        "scheduled_date" => "予定日",
        "due_date" => "締切日",
        _ => field,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_preserve_values_and_reject_incomplete_or_invented_evidence() {
        let policy = ItemOperationPolicy::new(
            "AufyのプロジェクトをAutomationにして",
            NaiveDate::from_ymd_opt(2026, 10, 7).unwrap(),
        );
        let raw = json!({"title":"Aufyの開発","reference":"Aufy","changes":[
            {"field":"project","value":"Automation","source":"プロジェクトをAutomationにして"}
        ]});
        assert_eq!(
            policy
                .validate("update_item", raw.clone())
                .unwrap()
                .arguments,
            json!({"title":"Aufyの開発","project":"Automation"})
        );
        for invalid in [
            json!({"field":"project","value":"Automation"}),
            json!({"field":"project","source":"プロジェクトをAutomationにして"}),
            json!({"field":"project","value":"Automation","source":"以前の会話"}),
            json!({"field":"external","value":"Automation","source":"Aufy"}),
        ] {
            let mut incomplete = raw.clone();
            incomplete["changes"][0] = invalid;
            assert!(policy.validate("update_item", incomplete).is_err());
        }
        let mut duplicate = raw.clone();
        duplicate["changes"]
            .as_array_mut()
            .unwrap()
            .push(raw["changes"][0].clone());
        assert!(policy.validate("update_item", duplicate).is_err());
        let mut from_snapshot = raw.clone();
        from_snapshot["reference"] = json!("Aufyの開発");
        assert!(policy
            .validate("update_item", from_snapshot)
            .unwrap()
            .reference
            .is_none());
        let mut empty = raw;
        empty["changes"] = json!([]);
        assert!(matches!(
            policy.validate("update_item", empty),
            Err(ValidationError::Clarification(_))
        ));
        assert!(policy.validate("external_tool", json!({})).is_err());
    }

    #[test]
    fn create_defaults_unspecified_attributes_and_requires_evidence_for_supplied_values() {
        let request = "butesにAufyの開発を入れて。低めの優先度で、メモにはgithubのtwil3akineのgwitgのURLを保存して";
        let policy =
            ItemOperationPolicy::new(request, NaiveDate::from_ymd_opt(2026, 10, 7).unwrap());
        let raw = json!({"title":"Aufyの開発","reference":"Aufyの開発","changes":[
            {"field":"kind","value":"bute","source":"butes"},
            {"field":"priority","value":"low","source":"低めの優先度"},
            {"field":"notes","value":"https://github.com/twil3akine/gwitg","source":"メモにはgithubのtwil3akineのgwitgのURLを保存して"}
        ]});
        let args = policy
            .validate("create_item", raw.clone())
            .unwrap()
            .arguments;
        assert_eq!(args["kind"], "bute");
        assert_eq!(args["priority"], "low");
        assert_eq!(args["notes"], "https://github.com/twil3akine/gwitg");
        assert!(args.get("project").is_none() && args.get("tags").is_none());
        for field in ["priority", "project", "tags", "notes"] {
            let mut guessed = raw.clone();
            guessed["changes"] =
                json!([{ "field":field,"value":"他Itemから推測","source":"以前の会話" }]);
            assert!(policy.validate("create_item", guessed).is_err());
        }
        let args = policy
            .validate(
                "create_item",
                json!({"title":"Aufyの開発","reference":"Aufyの開発","changes":[]}),
            )
            .unwrap()
            .arguments;
        assert_eq!(args["kind"], "task");
        assert!(args.get("priority").is_none() && args.get("notes").is_none());
    }

    #[test]
    fn relative_dates_resolve_across_year_boundary_and_invalid_dates_are_rejected() {
        let policy = ItemOperationPolicy::new(
            "レポートを今日開始、一週間後締切で追加して",
            NaiveDate::from_ymd_opt(2026, 12, 30).unwrap(),
        );
        let mut raw = json!({"title":"レポート","reference":"レポート","changes":[
            {"field":"scheduled_date","value":{"relative":"today"},"source":"今日開始"},
            {"field":"due_date","value":{"relative":"weeks_after","weeks":1},"source":"一週間後締切"}
        ]});
        let args = policy
            .validate("create_item", raw.clone())
            .unwrap()
            .arguments;
        assert_eq!(args["scheduled_date"], "2026-12-30");
        assert_eq!(args["due_date"], "2027-01-06");
        for invalid in [
            json!("2026-02-30"),
            json!({"relative":"unknown"}),
            json!({"relative":"days_after","days":u32::MAX}),
        ] {
            raw["changes"][1]["value"] = invalid;
            assert!(policy.validate("create_item", raw.clone()).is_err());
        }
        assert_eq!(
            resolve_date(policy.today, &json!({"relative":"next_week"})).unwrap(),
            "2027-01-04"
        );
    }
}
