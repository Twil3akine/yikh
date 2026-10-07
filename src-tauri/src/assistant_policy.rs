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
            for key in ["instruction", "reference"] {
                let source = args
                    .remove(key)
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .ok_or("操作と対象を、今回のユーザー発言から引用してください。")?;
                if !self.contains_source(&source) {
                    return Err("操作の根拠が今回のユーザー発言にありません。".into());
                }
            }
            let sources = args
                .remove("sources")
                .ok_or("指定項目の根拠を渡してください。")?;
            let sources = sources
                .as_object()
                .ok_or("指定項目の根拠はオブジェクトで渡してください。")?;
            for (field, source) in sources {
                if !MUTABLE_FIELDS.contains(&field.as_str()) {
                    return Err("指定項目の根拠に不明なフィールドがあります。".into());
                }
                if !source
                    .as_str()
                    .is_some_and(|source| self.contains_source(source))
                {
                    return Err("指定項目の根拠が今回のユーザー発言にありません。".into());
                }
            }
            // Omitted fields cannot inherit values guessed from other Items.
            // Existing patch handling preserves them on update; create uses defaults.
            for field in MUTABLE_FIELDS {
                if !sources.contains_key(*field) {
                    args.remove(*field);
                }
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
    fn structured_sources_control_fields_without_parsing_japanese_labels() {
        let request = "AufyのPJをAutomationにしてもらえるかな";
        let policy =
            ItemOperationPolicy::new(request, NaiveDate::from_ymd_opt(2026, 10, 7).unwrap());
        let mut raw = json!({"title":"Aufyの開発", "project":"Automation", "priority":"high", "notes":"他Itemから推測", "tags":["A"],
            "instruction":request, "reference":"Aufy", "sources":{"project":"PJをAutomationにして"}});
        let args = policy.validate("update_item", raw.clone()).unwrap();
        assert_eq!(args["project"], "Automation");
        for field in [
            "priority",
            "notes",
            "tags",
            "instruction",
            "reference",
            "sources",
        ] {
            assert!(args.get(field).is_none());
        }
        raw["sources"]["project"] = json!("以前の会話にある指定");
        assert!(policy.validate("update_item", raw).is_err());
        assert!(policy.validate("external_tool", json!({})).is_err());
    }

    #[test]
    fn create_uses_defaults_for_unspecified_attributes_and_accepts_sourced_notes() {
        let request = "butesにAufyの開発を入れて。低めの優先度で、メモにはgithubのtwil3akineのgwitgのURLを保存して";
        let policy =
            ItemOperationPolicy::new(request, NaiveDate::from_ymd_opt(2026, 10, 7).unwrap());
        let raw = json!({"title":"Aufyの開発", "kind":"bute", "priority":"low", "notes":"https://github.com/twil3akine/gwitg", "project":"A", "tags":["A"],
            "instruction":request, "reference":"Aufyの開発", "sources":{"kind":"butes", "priority":"低めの優先度", "notes":"メモにはgithubのtwil3akineのgwitgのURLを保存して"}});
        let args = policy.validate("create_item", raw.clone()).unwrap();
        assert_eq!(args["kind"], "bute");
        assert_eq!(args["priority"], "low");
        assert_eq!(args["notes"], "https://github.com/twil3akine/gwitg");
        assert!(args.get("project").is_none() && args.get("tags").is_none());
        let mut without_sources = raw;
        without_sources["sources"] = json!({});
        let args = policy.validate("create_item", without_sources).unwrap();
        assert_eq!(args["kind"], "task");
        assert!(args.get("priority").is_none() && args.get("notes").is_none());
    }

    #[test]
    fn relative_dates_resolve_across_year_boundary_and_invalid_dates_are_rejected() {
        let request = "レポートを今日開始、一週間後締切で追加して";
        let policy =
            ItemOperationPolicy::new(request, NaiveDate::from_ymd_opt(2026, 12, 30).unwrap());
        let mut raw = json!({"title":"レポート", "kind":"task", "scheduled_date":{"relative":"today"}, "due_date":{"relative":"days_after","days":7},
            "instruction":request, "reference":"レポート", "sources":{"scheduled_date":"今日開始", "due_date":"一週間後締切"}});
        let args = policy.validate("create_item", raw.clone()).unwrap();
        assert_eq!(args["scheduled_date"], "2026-12-30");
        assert_eq!(args["due_date"], "2027-01-06");
        for invalid in [
            json!("2026-02-30"),
            json!({"relative":"unknown"}),
            json!({"relative":"days_after","days":u32::MAX}),
        ] {
            raw["due_date"] = invalid;
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
