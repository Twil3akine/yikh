use chrono::{Datelike, Duration, NaiveDate};
use serde::Deserialize;
use serde_json::{json, Value};

const MUTABLE_FIELDS: &[&str] = &[
    "kind",
    "new_title",
    "priority",
    "project",
    "tags",
    "notes",
    "scheduled_date",
    "due_date",
];

/// The model interprets the request; Rust validates its structured arguments.
/// Evidence must come from this request, never from Item data or conversation history.
pub(crate) struct ItemOperationPolicy {
    request: String,
    today: NaiveDate,
}

impl ItemOperationPolicy {
    pub fn new(request: &str, today: NaiveDate) -> Self {
        Self {
            request: request.to_lowercase(),
            today,
        }
    }

    #[cfg(test)]
    pub fn request(&self) -> &str {
        &self.request
    }

    fn contains_source(&self, source: &str) -> bool {
        !source.trim().is_empty() && self.request.contains(&source.to_lowercase())
    }

    pub fn validate(&self, name: &str, arguments: Value) -> Result<Value, String> {
        if !matches!(
            name,
            "list_items" | "create_item" | "update_item" | "complete_item" | "delete_item"
        ) {
            return Err("利用できない操作です。".into());
        }
        let mut args = arguments
            .as_object()
            .cloned()
            .ok_or("Toolの引数はオブジェクトで指定してください。")?;
        if name != "list_items" {
            // The application already binds this policy to the current request.
            // Asking the model to restate that instruction adds no intent check.
            let reference = args
                .remove("reference")
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or("対象名を依頼文から引用してください。")?;
            if !self.contains_source(&reference) {
                return Err("Assistantが対象名を依頼文から正しく読み取れませんでした。Itemは変更していません。".into());
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
                    return Err("指定項目の根拠が今回のユーザー発言にありません。".into());
                }
                args.insert((*field).into(), attribute["value"].clone());
            }
            let title = args
                .get("title")
                .and_then(Value::as_str)
                .ok_or("対象のタイトルを指定してください。")?;
            if title.trim().is_empty() || (name == "create_item" && !self.contains_source(title)) {
                return Err("依頼に含まれるItemのタイトルを指定してください。".into());
            }
            if name == "create_item" && !args.contains_key("kind") {
                args.insert("kind".into(), json!("task"));
            }
            if name == "update_item" && args.len() == 1 {
                return Err(
                    "Assistantが更新内容を読み取れませんでした。Itemは変更していません。".into(),
                );
            }
        }
        for field in ["scheduled_date", "due_date", "due_from", "due_to"] {
            if name == "create_item" && args.get(field).is_some_and(Value::is_null) {
                args.remove(field);
                continue;
            }
            if let Some(value) = args.get(field) {
                args.insert(field.into(), self.resolve_date(value)?);
            }
        }
        Ok(Value::Object(args))
    }

    fn resolve_date(&self, value: &Value) -> Result<Value, String> {
        if value.is_null() {
            return Ok(Value::Null);
        }
        let date = if let Some(value) = value.as_str() {
            let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|_| "日付はYYYY-MM-DDまたは相対日付で指定してください。")?;
            if date.format("%Y-%m-%d").to_string() != value {
                return Err("日付はYYYY-MM-DDで指定してください。".into());
            }
            date
        } else {
            let relative: RelativeDate = serde_json::from_value(value.clone())
                .map_err(|_| "相対日付の形式が正しくありません。")?;
            relative.resolve(self.today)?
        };
        Ok(json!(date.format("%Y-%m-%d").to_string()))
    }
}

#[derive(Deserialize)]
#[serde(tag = "relative", rename_all = "snake_case", deny_unknown_fields)]
enum RelativeDate {
    Today,
    Tomorrow,
    DayAfterTomorrow,
    DaysAfter { days: u32 },
    NextWeek,
}

impl RelativeDate {
    fn resolve(&self, today: NaiveDate) -> Result<NaiveDate, String> {
        let days = match self {
            Self::Today => 0,
            Self::Tomorrow => 1,
            Self::DayAfterTomorrow => 2,
            Self::DaysAfter { days } => i64::from(*days),
            Self::NextWeek => 7 - i64::from(today.weekday().num_days_from_monday()),
        };
        today
            .checked_add_signed(Duration::days(days))
            .ok_or("指定された日付が範囲外です。".into())
    }
}

pub(crate) fn date_schema(nullable: bool) -> Value {
    let mut variants = vec![
        json!({"type":"string","description":"明示された日付 YYYY-MM-DD"}),
        json!({"type":"object","properties":{"relative":{"type":"string","enum":["today","tomorrow","day_after_tomorrow","next_week"]}},"required":["relative"],"additionalProperties":false}),
        json!({"type":"object","properties":{"relative":{"type":"string","enum":["days_after"]},"days":{"type":"integer","minimum":0}},"required":["relative","days"],"additionalProperties":false}),
    ];
    if nullable {
        variants.push(json!({"type":"null"}));
    }
    json!({"oneOf":variants,"description":"相対日付はRustが現在日から解決します。next_weekは次の月曜日。days_afterの7は一週間後です。"})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_attributes_preserve_requested_values_and_reject_incomplete_evidence() {
        let request = "AufyのプロジェクトをAutomationにしてもらえるかな";
        let policy =
            ItemOperationPolicy::new(request, NaiveDate::from_ymd_opt(2026, 10, 7).unwrap());
        let raw = json!({"title":"Aufyの開発",
            "project":{"value":"Automation","source":"プロジェクトをAutomationにして"},
            "reference":"Aufy"});
        let args = policy.validate("update_item", raw.clone()).unwrap();
        assert_eq!(args, json!({"title":"Aufyの開発","project":"Automation"}));
        for invalid in [
            json!({"value":"Automation"}),
            json!({"source":"プロジェクトをAutomationにして"}),
            json!({"value":"Automation","source":"以前の会話にある指定"}),
            json!("Automation"),
        ] {
            let mut incomplete = raw.clone();
            incomplete["project"] = invalid;
            assert!(policy.validate("update_item", incomplete).is_err());
        }
        let mut from_snapshot = raw.clone();
        from_snapshot["reference"] = json!("Aufyの開発");
        assert!(policy.validate("update_item", from_snapshot).is_err());
        let mut empty_update = raw;
        empty_update.as_object_mut().unwrap().remove("project");
        let error = policy.validate("update_item", empty_update).unwrap_err();
        assert!(error.contains("Assistantが更新内容を読み取れませんでした"));
        assert!(policy.validate("external_tool", json!({})).is_err());
    }

    #[test]
    fn create_defaults_unspecified_attributes_and_requires_evidence_for_supplied_values() {
        let request = "butesにAufyの開発を入れて。低めの優先度で、メモにはgithubのtwil3akineのgwitgのURLを保存して";
        let policy =
            ItemOperationPolicy::new(request, NaiveDate::from_ymd_opt(2026, 10, 7).unwrap());
        let raw = json!({"title":"Aufyの開発",
            "kind":{"value":"bute","source":"butes"},
            "priority":{"value":"low","source":"低めの優先度"},
            "notes":{"value":"https://github.com/twil3akine/gwitg","source":"メモにはgithubのtwil3akineのgwitgのURLを保存して"},
            "reference":"Aufyの開発"});
        let args = policy.validate("create_item", raw.clone()).unwrap();
        assert_eq!(args["kind"], "bute");
        assert_eq!(args["priority"], "low");
        assert_eq!(args["notes"], "https://github.com/twil3akine/gwitg");
        assert!(args.get("project").is_none() && args.get("tags").is_none());
        for field in ["priority", "project", "tags", "notes"] {
            let mut guessed = raw.clone();
            guessed[field] = json!({"value":"他Itemから推測","source":"以前の会話"});
            assert!(policy.validate("create_item", guessed).is_err());
        }
        let args = policy
            .validate(
                "create_item",
                json!({"title":"Aufyの開発",
            "reference":"Aufyの開発"}),
            )
            .unwrap();
        assert_eq!(args["kind"], "task");
        assert!(args.get("priority").is_none() && args.get("notes").is_none());
    }

    #[test]
    fn relative_dates_resolve_across_year_boundary_and_invalid_dates_are_rejected() {
        let request = "レポートを今日開始、一週間後締切で追加して";
        let policy =
            ItemOperationPolicy::new(request, NaiveDate::from_ymd_opt(2026, 12, 30).unwrap());
        let mut raw = json!({"title":"レポート",
            "scheduled_date":{"value":{"relative":"today"},"source":"今日開始"},
            "due_date":{"value":{"relative":"days_after","days":7},"source":"一週間後締切"},
            "reference":"レポート"});
        let args = policy.validate("create_item", raw.clone()).unwrap();
        assert_eq!(args["scheduled_date"], "2026-12-30");
        assert_eq!(args["due_date"], "2027-01-06");
        for invalid in [
            json!("2026-02-30"),
            json!({"relative":"unknown"}),
            json!({"relative":"days_after","days":u32::MAX}),
        ] {
            raw["due_date"]["value"] = invalid;
            assert!(policy.validate("create_item", raw.clone()).is_err());
        }
        for (relative, expected) in [
            (RelativeDate::Tomorrow, "2026-12-31"),
            (RelativeDate::DayAfterTomorrow, "2027-01-01"),
            (RelativeDate::NextWeek, "2027-01-04"),
        ] {
            assert_eq!(
                relative.resolve(policy.today).unwrap().to_string(),
                expected
            );
        }
    }
}
