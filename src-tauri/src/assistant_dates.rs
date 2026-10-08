use crate::assistant_routing::Change;
use chrono::{Datelike, Duration, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) const DATE_FIELDS: &[&str] = &["scheduled_date", "due_date"];

/// Extracted from the current request before the model sees Item candidates.
/// The later Tool call must preserve these values instead of interpreting them again.
#[derive(Debug, Default, Serialize)]
pub(crate) struct RequestedDates {
    pub dates: Vec<Change>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DateExtraction {
    dates: Vec<DateExpression>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DateExpression {
    field: String,
    source: String,
    mode: String,
    unit: String,
    amount: u32,
    date: String,
}

impl DateExpression {
    fn into_change(self) -> Result<Change, String> {
        let value = if self.mode == "offset" {
            if self.amount == 0 || !self.date.is_empty() {
                return Err("相対日付の単位・移動量を確認できませんでした。".into());
            }
            let (relative, key) = match self.unit.as_str() {
                "day" => ("days_after", "days"),
                "week" => ("weeks_after", "weeks"),
                "month" => ("months_after", "months"),
                "year" => ("years_after", "years"),
                _ => return Err("相対日付の単位を確認できませんでした。".into()),
            };
            let mut value = json!({"relative":relative});
            value[key] = json!(self.amount);
            value
        } else {
            if self.amount != 0
                || self.unit != "none"
                || (self.mode != "absolute" && !self.date.is_empty())
            {
                return Err("日付の形式と値が一致しません。".into());
            }
            match self.mode.as_str() {
                "today" | "tomorrow" | "day_after_tomorrow" | "next_week" => {
                    json!({"relative":self.mode})
                }
                "absolute" => json!(self.date),
                "clear" => Value::Null,
                _ => return Err("日付の指定方法を確認できませんでした。".into()),
            }
        };
        Ok(Change {
            field: self.field,
            source: self.source,
            value,
        })
    }
}

impl RequestedDates {
    pub fn schema(fields: &[&str]) -> Value {
        json!({"type":"object","properties":{"dates":{
            "type":"array","minItems":fields.len(),"maxItems":fields.len(),
            "items":{"type":"object","properties":{
                "field":{"type":"string","enum":fields},
                "source":{"type":"string","minLength":1},
                "mode":{"type":"string","enum":["offset","today","tomorrow","day_after_tomorrow","next_week","absolute","clear"]},
                "unit":{"type":"string","enum":["day","week","month","year","none"]},
                "amount":{"type":"integer","minimum":0},
                "date":{"type":"string"}
            },"required":["field","source","mode","unit","amount","date"],"additionalProperties":false}
        }},"required":["dates"],"additionalProperties":false})
    }

    pub fn parse(
        content: &str,
        request: &str,
        fields: &[&str],
        today: NaiveDate,
    ) -> Result<Self, String> {
        let extraction: DateExtraction = serde_json::from_str(content)
            .map_err(|_| "日付の指定を読み取れませんでした。".to_owned())?;
        let plan = Self {
            dates: extraction
                .dates
                .into_iter()
                .map(DateExpression::into_change)
                .collect::<Result<_, _>>()?,
        };
        let mut seen = HashSet::new();
        for date in &plan.dates {
            if !fields.contains(&date.field.as_str()) || !seen.insert(date.field.as_str()) {
                return Err("日付の項目が未対応または重複しています。".into());
            }
            if date.source.trim().is_empty()
                || !request.to_lowercase().contains(&date.source.to_lowercase())
            {
                return Err("日付の根拠を今回の発言から確認できませんでした。".into());
            }
            resolve_date(today, &date.value)?;
        }
        if seen.len() != fields.len() {
            return Err("指定された日付をすべて確認できませんでした。".into());
        }
        Ok(plan)
    }

    pub fn constrain_tools(&self, definitions: &mut Value) -> Result<(), String> {
        if self.dates.is_empty() {
            return Ok(());
        }
        let variants = definitions[0]["function"]["parameters"]["properties"]["changes"]["items"]
            ["oneOf"]
            .as_array_mut()
            .ok_or("日付のTool定義を利用できません。")?;
        for date in &self.dates {
            let variant = variants
                .iter_mut()
                .find(|variant| variant["properties"]["field"]["enum"][0] == date.field)
                .ok_or("指定された日付のTool定義がありません。")?;
            variant["properties"]["value"] = json!({"enum":[date.value]});
            variant["properties"]["source"] = json!({"type":"string","enum":[date.source]});
        }
        Ok(())
    }

    pub fn validate_change(&self, field: &str, value: &Value) -> Result<(), String> {
        let expected = self
            .dates
            .iter()
            .find(|date| date.field == field)
            .ok_or("今回指定されていない日付は変更できません。")?;
        if expected.value != *value {
            return Err(format!("{field}が今回の日付計画と一致しません。valueは{}をそのまま使ってください。Itemは変更していません。", expected.value));
        }
        Ok(())
    }
}

/// Resolve a validated explicit date or relative-date object from the given day.
pub(crate) fn resolve_date(today: NaiveDate, value: &Value) -> Result<Value, String> {
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
        relative.resolve(today)?
    };

    if !(0..=9999).contains(&date.year()) {
        return Err("指定された日付が範囲外です。".into());
    }
    Ok(json!(date.format("%Y-%m-%d").to_string()))
}

pub(crate) fn date_schema(nullable: bool) -> Value {
    let mut variants = vec![
        json!({"type":"string","description":"明示された日付 YYYY-MM-DD"}),
        json!({"type":"object","properties":{"relative":{"type":"string","enum":["today","tomorrow","day_after_tomorrow","next_week"]}},"required":["relative"],"additionalProperties":false}),
        json!({"type":"object","properties":{"relative":{"type":"string","enum":["days_after"]},"days":{"type":"integer","minimum":0}},"required":["relative","days"],"additionalProperties":false}),
        json!({"type":"object","properties":{"relative":{"type":"string","enum":["weeks_after"]},"weeks":{"type":"integer","minimum":0}},"required":["relative","weeks"],"additionalProperties":false}),
        json!({"type":"object","properties":{"relative":{"type":"string","enum":["months_after"]},"months":{"type":"integer","minimum":0}},"required":["relative","months"],"additionalProperties":false}),
        json!({"type":"object","properties":{"relative":{"type":"string","enum":["years_after"]},"years":{"type":"integer","minimum":0}},"required":["relative","years"],"additionalProperties":false}),
    ];
    if nullable {
        variants.push(json!({"type":"null"}));
    }
    json!({"oneOf":variants,"description":"相対日付はRustが現在日から解決します。next_weekは次の月曜日。days_afterの7は一週間後です。months_afterとyears_afterは暦上の日付が変わる場合はエラーになります。"})
}

#[derive(Deserialize)]
#[serde(tag = "relative", rename_all = "snake_case", deny_unknown_fields)]
enum RelativeDate {
    Today,
    Tomorrow,
    DayAfterTomorrow,
    DaysAfter { days: u32 },
    WeeksAfter { weeks: u32 },
    MonthsAfter { months: u32 },
    YearsAfter { years: u32 },
    NextWeek,
}

impl RelativeDate {
    fn resolve(&self, today: NaiveDate) -> Result<NaiveDate, String> {
        let days = match self {
            Self::Today => 0,
            Self::Tomorrow => 1,
            Self::DayAfterTomorrow => 2,
            Self::DaysAfter { days } => i64::from(*days),
            Self::WeeksAfter { weeks } => i64::from(*weeks)
                .checked_mul(7)
                .ok_or("指定された日付が範囲外です。")?,
            Self::MonthsAfter { months } => {
                return add_calendar_months(today, *months);
            }
            Self::YearsAfter { years } => {
                let months = years
                    .checked_mul(12)
                    .ok_or("指定された日付が範囲外です。")?;
                return add_calendar_months(today, months);
            }
            Self::NextWeek => 7 - i64::from(today.weekday().num_days_from_monday()),
        };
        today
            .checked_add_signed(Duration::days(days))
            .ok_or("指定された日付が範囲外です。".into())
    }
}

fn add_calendar_months(today: NaiveDate, months: u32) -> Result<NaiveDate, String> {
    let date = today
        .checked_add_months(Months::new(months))
        .ok_or("指定された日付が範囲外です。")?;
    if date.day() != today.day() {
        return Err("暦上の日付が存在しないため、相対日付を解決できません。".into());
    }
    Ok(date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_dates_require_current_sources_complete_fields_and_preserve_values() {
        let today = date(2026, 10, 8);
        let request = "予定は明日、締切を1ヶ月後にして";
        let data = json!({"dates":[
            {"field":"scheduled_date","mode":"tomorrow","unit":"none","amount":0,"date":"","source":"予定は明日"},
            {"field":"due_date","mode":"offset","unit":"month","amount":1,"date":"","source":"締切を1ヶ月後"}
        ]});
        let fields = ["scheduled_date", "due_date"];
        let plan = RequestedDates::parse(&data.to_string(), request, &fields, today).unwrap();
        assert_eq!(
            resolve_date(today, &plan.dates[1].value).unwrap(),
            "2026-11-08"
        );
        for value in [
            json!({"relative":"today"}),
            json!({"relative":"weeks_after","weeks":0}),
            json!("2026-10-08"),
            Value::Null,
        ] {
            assert!(plan.validate_change("due_date", &value).is_err());
        }
        assert!(plan
            .validate_change("due_date", &json!({"relative":"months_after","months":1}))
            .is_ok());
        let mut invalid = data.clone();
        invalid["dates"].as_array_mut().unwrap().pop();
        assert!(RequestedDates::parse(&invalid.to_string(), request, &fields, today).is_err());
        let mut invalid = data.clone();
        invalid["dates"][1]["source"] = json!("前の依頼");
        assert!(RequestedDates::parse(&invalid.to_string(), request, &fields, today).is_err());
        let mut invalid = data.clone();
        invalid["dates"][1]["field"] = json!("scheduled_date");
        assert!(RequestedDates::parse(&invalid.to_string(), request, &fields, today).is_err());
        let mut invalid = data;
        invalid["dates"][1]["amount"] = json!(0);
        assert!(RequestedDates::parse(&invalid.to_string(), request, &fields, today).is_err());
        for (request, mode, explicit, expected) in [
            ("締切なし", "clear", "", Value::Null),
            ("締切は今日", "today", "", json!({"relative":"today"})),
            (
                "締切は2026-11-08",
                "absolute",
                "2026-11-08",
                json!("2026-11-08"),
            ),
        ] {
            let data = json!({"dates":[{"field":"due_date","mode":mode,"unit":"none","amount":0,"date":explicit,"source":request}]});
            let plan =
                RequestedDates::parse(&data.to_string(), request, &["due_date"], today).unwrap();
            assert_eq!(plan.dates[0].value, expected);
        }
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn resolve(today: NaiveDate, relative: Value) -> Result<Value, String> {
        resolve_date(today, &relative)
    }

    #[test]
    fn resolves_relative_days_and_calendar_units() {
        let today = date(2026, 10, 7);
        for (relative, expected) in [
            (json!({"relative":"today"}), "2026-10-07"),
            (json!({"relative":"tomorrow"}), "2026-10-08"),
            (json!({"relative":"day_after_tomorrow"}), "2026-10-09"),
            (json!({"relative":"days_after","days":7}), "2026-10-14"),
            (json!({"relative":"weeks_after","weeks":1}), "2026-10-14"),
            (json!({"relative":"months_after","months":1}), "2026-11-07"),
            (json!({"relative":"years_after","years":1}), "2027-10-07"),
        ] {
            assert_eq!(resolve(today, relative).unwrap(), expected);
        }
        assert_eq!(resolve_date(today, &Value::Null).unwrap(), Value::Null);
    }

    #[test]
    fn rejects_calendar_clamping_and_date_overflow() {
        assert!(resolve(
            date(2024, 2, 29),
            json!({"relative":"years_after","years":1})
        )
        .is_err());
        assert!(resolve(
            date(2026, 1, 31),
            json!({"relative":"months_after","months":1})
        )
        .is_err());
        assert!(resolve(
            date(9999, 12, 31),
            json!({"relative":"days_after","days":1})
        )
        .is_err());
        assert!(resolve(
            date(9999, 12, 31),
            json!({"relative":"months_after","months":1})
        )
        .is_err());
        assert!(resolve(
            date(2026, 10, 7),
            json!({"relative":"years_after","years":u32::MAX})
        )
        .is_err());
    }
}
