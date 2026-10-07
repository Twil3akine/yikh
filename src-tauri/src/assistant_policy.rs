use chrono::{Datelike, Duration, NaiveDate};
use serde::Deserialize;
use serde_json::{json, Value};

/// Rules apply to the current user request, never to the Item snapshot or model history.
pub(crate) struct ItemOperationPolicy {
    request: String,
    today: NaiveDate,
    operation: Option<&'static str>,
}

impl ItemOperationPolicy {
    pub fn new(request: &str, today: NaiveDate) -> Self {
        let request = request.to_lowercase();
        let operations = [
            (
                "create_item",
                &[
                    "追加して",
                    "追加お願い",
                    "追加したい",
                    "登録して",
                    "作成して",
                    "作って",
                    "入れて",
                    "入れといて",
                    "登録お願い",
                    "add ",
                    "create ",
                ][..],
            ),
            (
                "update_item",
                &[
                    "変更して",
                    "変更お願い",
                    "更新して",
                    "修正して",
                    "にして",
                    "update ",
                    "change ",
                    "set ",
                ][..],
            ),
            (
                "complete_item",
                &[
                    "終わった",
                    "終わりました",
                    "できた",
                    "できました",
                    "完了した",
                    "完了しました",
                    "完了して",
                    "完了にして",
                    "complete ",
                    "finished ",
                ][..],
            ),
            (
                "delete_item",
                &["削除して", "削除お願い", "消して", "delete ", "remove "][..],
            ),
        ];
        let mut matched: Vec<_> = operations
            .iter()
            .filter(|(_, phrases)| phrases.iter().any(|phrase| request.contains(phrase)))
            .map(|(name, _)| *name)
            .collect();
        if matched.contains(&"complete_item") && request.contains("完了にして") {
            matched.retain(|name| *name != "update_item");
        }
        let completion_request = ["完了にして", "完了してください", "complete "]
            .iter()
            .any(|phrase| request.contains(phrase));
        let status_question = !completion_request
            && (request.contains(['?', '？'])
                || [
                    "終わったか",
                    "終わりましたか",
                    "終わったら",
                    "完了したか",
                    "完了しましたか",
                    "完了したら",
                    "完了している",
                    "完了してる",
                    "できたか",
                    "できましたか",
                    "できたら",
                    "よね",
                    "ですか",
                ]
                .iter()
                .any(|phrase| request.contains(phrase)));
        if status_question {
            matched.retain(|name| *name != "complete_item");
        }
        // Descriptions and questions about operations do not authorize a write.
        let informational = [
            "方法を教",
            "やり方を教",
            "教えて",
            "見せて",
            "一覧を見",
            "相談したい",
            "追加してある",
            "追加している",
            "変更してある",
            "削除してある",
            "にしてある",
            "にしている",
            "更新してある",
            "追加しない",
            "登録しない",
            "作成しない",
            "変更しない",
            "更新しない",
            "完了しない",
            "完了にしない",
            "削除しない",
            "消さない",
            "ことにしない",
            "という",
            "って言",
        ]
        .iter()
        .any(|word| request.contains(word));
        let operation = if matched.len() == 1 && !informational {
            Some(matched[0])
        } else {
            None
        };
        Self {
            request,
            today,
            operation,
        }
    }

    pub fn allows(&self, name: &str) -> bool {
        name == "list_items" || self.operation == Some(name)
    }

    pub fn tool_choice(&self) -> Value {
        // llama.cpp accepts string choices. Named function objects can fall back
        // to "auto", so pair "required" with just the requested tool definition.
        json!(if self.requires_operation() {
            "required"
        } else {
            "auto"
        })
    }

    pub fn requires_operation(&self) -> bool {
        self.operation.is_some()
    }

    pub fn request(&self) -> &str {
        &self.request
    }

    pub fn validate(&self, name: &str, arguments: Value) -> Result<Value, String> {
        if !self.allows(name) {
            return Err("今回の依頼では、このItem操作を実行できません。".into());
        }
        let mut args = arguments
            .as_object()
            .cloned()
            .ok_or("Toolの引数はオブジェクトで指定してください。")?;
        let mut field_policy = Self {
            request: self.request.clone(),
            today: self.today,
            operation: self.operation,
        };
        if name != "list_items" {
            let title = args
                .get("title")
                .and_then(Value::as_str)
                .ok_or("対象のタイトルを指定してください。")?;
            if title.trim().is_empty()
                || (name == "create_item" && !self.request.contains(&title.to_lowercase()))
            {
                return Err("依頼に含まれるItemのタイトルを指定してください。".into());
            }
            // Existing targets, including shortened names, are independently resolved
            // against the current request by the tool dispatcher before any write.
            // A word in the target title is not a request to set that attribute.
            field_policy.request = self.request.replacen(&title.to_lowercase(), "", 1);
            // Ignore model-inferred optional attributes. Create uses the GUI defaults;
            // update retains existing values through its existing patch implementation.
            for field in ["priority", "project", "tags", "notes"] {
                if args
                    .get(field)
                    .is_some_and(|value| !field_policy.explicit_attribute(field, value))
                {
                    args.remove(field);
                }
            }
            if let Some(title) = args.get("new_title").and_then(Value::as_str) {
                if !self.request.contains(&title.to_lowercase()) {
                    return Err("変更後のタイトルが依頼に含まれていません。".into());
                }
            }
            if let Some(kind) = args.get("kind").and_then(Value::as_str) {
                let bute = field_policy.request.contains("bute");
                let task = field_policy.request.contains("task")
                    || field_policy.request.contains("タスク");
                if (bute && kind != "bute") || (task && !bute && kind != "task") {
                    return Err("Itemの種類が依頼と一致しません。".into());
                }
                let kind_change = field_policy.has_labeled_value(
                    &["種類", "種別", "type"],
                    &[kind, if kind == "task" { "タスク" } else { "bute" }],
                ) || [
                    format!("{kind}にして"),
                    format!("{kind}に変更"),
                    format!("{kind}に変え"),
                    "タスクにして".into(),
                    "タスクに変更".into(),
                ]
                .iter()
                .any(|text| field_policy.request.contains(text));
                if name == "update_item" && !kind_change {
                    args.remove("kind");
                } else if name == "create_item" && !task && !bute {
                    args.insert("kind".into(), json!("task"));
                }
            }
        }
        for field in ["scheduled_date", "due_date", "due_from", "due_to"] {
            if name != "list_items" && !field_policy.date_field_requested(field) {
                args.remove(field);
                continue;
            }
            if name == "create_item" && args.get(field).is_some_and(Value::is_null) {
                args.remove(field);
                continue;
            }
            if let Some(value) = args.get(field) {
                let date = field_policy.resolve_date(value, name != "list_items", field)?;
                args.insert(field.into(), date);
            }
        }
        Ok(Value::Object(args))
    }

    fn date_field_requested(&self, field: &str) -> bool {
        let labels: &[&str] = if field == "scheduled_date" {
            &["予定", "やる日", "開始", "着手", "scheduled", "start"]
        } else {
            &[
                "締切",
                "締め切り",
                "〆切",
                "期限",
                "まで",
                "due",
                "deadline",
            ]
        };
        labels.iter().any(|label| self.request.contains(label))
    }

    fn explicit_attribute(&self, field: &str, value: &Value) -> bool {
        let labels: &[&str] = match field {
            "priority" => &["優先度", "priority"],
            "project" => &["プロジェクト", "project"],
            "tags" => &["タグ", "tags", "tag"],
            "notes" => &["メモ", "ノート", "notes", "note"],
            _ => return false,
        };
        if field == "notes" {
            if let Some(reference) = value.as_str().and_then(github_reference) {
                // A URL assembled from an explicit owner/repository reference is
                // still grounded in this request; this does not look up a repository.
                if self.has_labeled_value(labels, &[&reference]) {
                    return true;
                }
            }
        }
        let values: Vec<&str> = match (field, value) {
            ("priority", Value::String(value)) => match value.as_str() {
                "high" => vec!["high", "高", "高め", "高い"],
                "medium" => vec!["medium", "中", "普通"],
                "low" => vec!["low", "低", "低め", "低い"],
                "none" => vec!["none", "なし", "未設定", "解除"],
                _ => return false,
            },
            (_, Value::String(value)) if !value.is_empty() => vec![value],
            (_, Value::Null) | (_, Value::String(_)) => {
                vec!["なし", "未設定", "解除", "外して", "消して", "空"]
            }
            ("tags", Value::Array(values)) => {
                if values.is_empty() {
                    return self.has_labeled_value(labels, &["なし", "解除", "外して", "空"]);
                }
                return values.iter().all(|value| {
                    value.as_str().is_some_and(|tag| {
                        !tag.is_empty()
                            && (self.request.contains(&format!("#{}", tag.to_lowercase()))
                                || self.has_labeled_value(labels, &[tag]))
                    })
                });
            }
            _ => return false,
        };
        self.has_labeled_value(labels, &values)
    }

    fn has_labeled_value(&self, labels: &[&str], values: &[&str]) -> bool {
        self.request.split(['。', '\n', ';', '；']).any(|clause| {
            if [
                "他の",
                "別の",
                "類推",
                "推測",
                "引き継",
                "しない",
                "ではなく",
                "じゃなく",
                "不要",
            ]
            .iter()
            .any(|word| clause.contains(word))
            {
                return false;
            }
            let clause = compact(clause);
            labels.iter().any(|label| {
                clause.match_indices(label).any(|(start, _)| {
                    let rest = clause[start + label.len()..]
                        .trim_start_matches([':', '：', '=', 'は', 'を', 'に', 'で']);
                    let entries: Vec<_> = if labels.contains(&"タグ") {
                        rest.split([',', '、', 'と', '/', '／']).collect()
                    } else {
                        vec![rest]
                    };
                    values.iter().any(|value| {
                        entries.iter().any(|entry| {
                            entry.strip_prefix(&compact(value)).is_some_and(|tail| {
                                tail.is_empty()
                                    || tail.starts_with([
                                        '、', ',', '。', '/', '／', 'に', 'で', 'と', 'を', 'が',
                                    ])
                                    || tail.starts_with("です")
                            })
                        })
                    })
                }) || values
                    .iter()
                    .any(|value| clause.contains(&format!("{}{label}", compact(value))))
            })
        })
    }

    fn resolve_date(
        &self,
        value: &Value,
        require_source: bool,
        field: &str,
    ) -> Result<Value, String> {
        if value.is_null() {
            if field == "due_date" && self.request.contains("無期限") {
                return Ok(Value::Null);
            }
            let labels: &[&str] = if field == "scheduled_date" {
                &["予定", "予定日", "開始", "scheduled"]
            } else {
                &["締切", "締切日", "期限", "due", "deadline"]
            };
            if require_source
                && !self.has_labeled_value(
                    labels,
                    &["なし", "未設定", "解除", "外して", "消して", "空"],
                )
            {
                return Err("日付の解除が依頼に含まれていません。".into());
            }
            return Ok(Value::Null);
        }
        let (date, source_matches) = if let Some(value) = value.as_str() {
            let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|_| "日付はYYYY-MM-DDまたは相対日付で指定してください。")?;
            if date.format("%Y-%m-%d").to_string() != value {
                return Err("日付はYYYY-MM-DDで指定してください。".into());
            }
            let explicit = [
                value.to_owned(),
                date.format("%Y/%m/%d").to_string(),
                format!("{}/{}/{}", date.year(), date.month(), date.day()),
                format!("{}年{}月{}日", date.year(), date.month(), date.day()),
            ]
            .iter()
            .any(|text| date_token_in(&self.request, text));
            let month_day = date.year() == self.today.year()
                && [
                    format!("{}/{}", date.month(), date.day()),
                    date.format("%m/%d").to_string(),
                    format!("{}月{}日", date.month(), date.day()),
                ]
                .iter()
                .any(|text| date_token_in(&self.request, text));
            (date, explicit || month_day)
        } else {
            let relative: RelativeDate = serde_json::from_value(value.clone())
                .map_err(|_| "相対日付の形式が正しくありません。")?;
            (
                relative.resolve(self.today)?,
                relative.matches(&self.request),
            )
        };
        if require_source && !source_matches {
            return Err(
                "依頼にない日付は設定できません。相対日付はtodayなどの形式を使ってください。"
                    .into(),
            );
        }
        Ok(json!(date.format("%Y-%m-%d").to_string()))
    }
}

fn github_reference(value: &str) -> Option<String> {
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let segments: Vec<_> = url.path_segments()?.collect();
    if segments.len() != 2
        || segments.iter().any(|segment| {
            segment.is_empty()
                || *segment == "."
                || *segment == ".."
                || !segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || ['-', '_', '.'].contains(&c))
        })
    {
        return None;
    }
    if value != format!("https://github.com/{}/{}", segments[0], segments[1]) {
        return None;
    }
    Some(format!(
        "githubの{}の{}のリポジトリのurl",
        segments[0], segments[1]
    ))
}

fn compact(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace() && !['「', '」', '『', '』', '\'', '"'].contains(c))
        .collect()
}

fn date_token_in(request: &str, token: &str) -> bool {
    request.match_indices(token).any(|(start, _)| {
        let before = request[..start].chars().next_back();
        let after = request[start + token.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_digit() || ['/', '-', '年'].contains(&c))
            && !after.is_some_and(|c| c.is_ascii_digit() || ['/', '-'].contains(&c))
    })
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

    fn matches(&self, request: &str) -> bool {
        match self {
            Self::Today => request.contains("今日") || request.contains("today"),
            Self::Tomorrow => {
                request.contains("明日")
                    || (request.contains("tomorrow") && !request.contains("day after tomorrow"))
            }
            Self::DayAfterTomorrow => {
                request.contains("明後日") || request.contains("day after tomorrow")
            }
            Self::NextWeek => request.contains("来週") || request.contains("next week"),
            Self::DaysAfter { days } => {
                request.contains(&format!("{days}日後"))
                    || request.contains(&format!("{days} days"))
                    || (*days == 7
                        && [
                            "一週間後",
                            "1週間後",
                            "１週間後",
                            "1週後",
                            "一週後",
                            "a week",
                            "one week",
                        ]
                        .iter()
                        .any(|text| request.contains(text)))
            }
        }
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

    fn policy(request: &str) -> ItemOperationPolicy {
        ItemOperationPolicy::new(request, NaiveDate::from_ymd_opt(2026, 12, 30).unwrap())
    }

    #[test]
    fn relative_dates_resolve_across_year_boundary_and_reject_invented_dates() {
        let p = policy("レポートを今日開始、一週間後締切で追加して");
        let args = p.validate("create_item", json!({"title":"レポート","kind":"task","scheduled_date":{"relative":"today"},"due_date":{"relative":"days_after","days":7}})).unwrap();
        assert_eq!(args["scheduled_date"], "2026-12-30");
        assert_eq!(args["due_date"], "2027-01-06");
        assert!(p
            .validate(
                "create_item",
                json!({"title":"レポート","kind":"task","due_date":"2027-01-06"})
            )
            .is_err());
        for (relative, expected) in [
            (RelativeDate::Tomorrow, "2026-12-31"),
            (RelativeDate::DayAfterTomorrow, "2027-01-01"),
            (RelativeDate::NextWeek, "2027-01-04"),
        ] {
            assert_eq!(relative.resolve(p.today).unwrap().to_string(), expected);
        }
    }

    #[test]
    fn explicit_repository_note_keeps_only_the_requested_github_url() {
        let p = policy("butesにAufyの開発を入れてもらえるかな。予定日はなしで優先度低め、メモにgithubのtwil3akineのgwitgのリポジトリのURLを貼っておいて");
        assert!(p.requires_operation());
        for (notes, accepted) in [
            ("https://github.com/Twil3akine/gwitg", true),
            ("https://github.com/Twil3akine/yikh", false),
            ("https://github.com/another/gwitg", false),
            ("https://example.com/Twil3akine/gwitg", false),
            ("https://github.com/Twil3akine/gwitg?extra=1", false),
            ("https://github.com/Twil3akine/gwitg/issues", false),
        ] {
            let args = p.validate("create_item", json!({"title":"Aufyの開発","kind":"bute","scheduled_date":null,"priority":"low","notes":notes,"project":"A","tags":["A"]})).unwrap();
            assert_eq!(
                args.get("notes").and_then(Value::as_str),
                accepted.then_some(notes)
            );
            assert_eq!(args["priority"], "low");
            assert!(args.get("scheduled_date").is_none());
            assert!(args.get("project").is_none() && args.get("tags").is_none());
        }
        let p = policy("Aufyの開発をButeで追加して");
        let args = p.validate("create_item", json!({"title":"Aufyの開発","kind":"bute","notes":"https://github.com/Twil3akine/gwitg"})).unwrap();
        assert!(args.get("notes").is_none());
    }

    #[test]
    fn unrequested_attributes_and_tools_are_not_authorized() {
        let p = policy("OSS課題レポートをTaskで追加して");
        let args = p.validate("create_item", json!({"title":"OSS課題レポート","kind":"task","priority":"high","project":"大学","tags":["研究"],"notes":"他の課題から推測"})).unwrap();
        assert!(
            args.get("priority").is_none()
                && args.get("project").is_none()
                && args.get("tags").is_none()
                && args.get("notes").is_none()
        );
        assert!(p
            .validate("delete_item", json!({"title":"OSS課題レポート"}))
            .is_err());
        assert!(!policy("今何をやるべき？").allows("create_item"));
        assert!(!policy("追加してくださいという文を直して").allows("create_item"));
        for request in [
            "OSS課題レポート終わった？",
            "OSS課題レポートは完了しましたか",
            "OSS課題レポートが終わったら何をすればいい？",
            "OSS課題レポートを終わったことにしないで",
        ] {
            assert!(!policy(request).allows("complete_item"), "{request}");
        }
        assert!(policy("OSS課題レポートを完了にしてもらえる？").allows("complete_item"));
        let explicit =
            policy("レポートを追加して。優先度はHigh、プロジェクトは大学、メモは実験、タグは研究");
        let args = explicit.validate("create_item", json!({"title":"レポート","kind":"task","priority":"high","project":"大学","notes":"実験","tags":["研究"]})).unwrap();
        assert_eq!(args["priority"], "high");
        assert_eq!(args["project"], "大学");
        assert_eq!(args["notes"], "実験");
        assert_eq!(args["tags"], json!(["研究"]));
    }
}
