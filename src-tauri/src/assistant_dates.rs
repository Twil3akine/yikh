use chrono::{Datelike, Duration, Months, NaiveDate};
use serde::Deserialize;
use serde_json::{json, Value};

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
