use crate::model::Item;

/// Resolve an assistant's title hint against the titles explicitly mentioned by the user.
///
/// A complete title in the request is authoritative and returns every item with that
/// title, including single-word titles. Abbreviated references need at least two
/// ordered words; generic words alone cannot identify a different stored title.
pub(crate) fn resolve_targets(
    items: Vec<Item>,
    request: &str,
    tool_title: &str,
) -> Result<Vec<Item>, String> {
    let request_normalized = normalize(request);
    let tool_normalized = normalize(tool_title);
    if request_normalized.is_empty() || tool_normalized.is_empty() {
        return Err(explicit_title_error());
    }
    let target_end = [
        "の締切",
        "の期限",
        "の予定",
        "の優先度",
        "のプロジェクト",
        "のタグ",
        "のメモ",
    ]
    .iter()
    .filter_map(|marker| request.find(marker))
    .min()
    .unwrap_or(request.len());
    let request_terms = meaningful_terms(&request[..target_end]);

    let exact_title_items: Vec<Item> = items
        .iter()
        .filter(|item| {
            let title = normalize(&item.title);
            let title_terms = meaningful_terms(&item.title);
            !title_terms.is_empty()
                && title_terms == request_terms
                && contains_phrase(&request_normalized, &title)
        })
        .filter(|item| normalize(&item.title) == tool_normalized)
        .cloned()
        .collect();
    if !exact_title_items.is_empty() {
        return Ok(exact_title_items);
    }

    if !enough_reference_terms(&request_terms) {
        return Err(explicit_title_error());
    }

    // The model's title hint may be either a short alias or the canonical stored title,
    // but it must still be supported by the words in the user's request.
    let tool_terms = meaningful_terms(tool_title);
    if !terms_match_title(&request_terms, tool_title)
        && !(enough_reference_terms(&tool_terms) && terms_match_title(&tool_terms, request))
    {
        return Err(explicit_title_error());
    }

    let matches: Vec<Item> = items
        .into_iter()
        .filter(|item| terms_match_title(&request_terms, &item.title))
        .collect();

    if matches.is_empty() {
        Err(explicit_title_error())
    } else {
        Ok(matches)
    }
}

fn explicit_title_error() -> String {
    "対象を特定できませんでした。項目のタイトルを明示してください。".to_string()
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|ch| !ch.is_whitespace() && !is_quote_or_punctuation(*ch))
        .collect()
}

fn is_quote_or_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '"' | '\''
            | '“'
            | '”'
            | '「'
            | '」'
            | '『'
            | '』'
            | '（'
            | '）'
            | '('
            | ')'
            | '、'
            | '。'
            | ','
            | '.'
            | '！'
            | '!'
            | '？'
            | '?'
            | ':'
            | '：'
            | '；'
            | ';'
            | '・'
            | '〜'
            | '～'
            | '—'
            | '–'
            | '-'
            | '_'
            | '／'
            | '/'
    )
}

fn contains_phrase(haystack: &str, needle: &str) -> bool {
    !needle.is_empty() && haystack.contains(needle)
}

fn meaningful_terms(value: &str) -> Vec<String> {
    tokenize(value)
        .into_iter()
        .filter(|term| !is_stop_term(term))
        .collect()
}

fn enough_reference_terms(terms: &[String]) -> bool {
    terms.len() >= 2 && terms.iter().any(|term| !is_generic_term(term))
}

fn terms_match_title(terms: &[String], title: &str) -> bool {
    if terms.is_empty() {
        return false;
    }
    let title_terms = tokenize(title);
    let mut next = 0;
    for term in terms {
        let Some(found) = title_terms[next..]
            .iter()
            .position(|title_term| title_term == term)
        else {
            return false;
        };
        next += found + 1;
    }
    true
}

fn tokenize(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut current_class = 0;

    for ch in normalize(value).chars() {
        let class = character_class(ch);
        if class == 0 {
            if !current.is_empty() {
                terms.push(std::mem::take(&mut current));
            }
            current_class = 0;
            continue;
        }
        if current_class != 0 && current_class != class {
            terms.push(std::mem::take(&mut current));
        }
        current_class = class;
        current.push(ch);
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
}

fn character_class(ch: char) -> u8 {
    if ch.is_ascii_alphanumeric() {
        1
    } else if ('\u{3040}'..='\u{309f}').contains(&ch) {
        2
    } else if ('\u{30a0}'..='\u{30ff}').contains(&ch) {
        3
    } else if ('\u{3400}'..='\u{9fff}').contains(&ch) {
        4
    } else {
        0
    }
}

fn is_stop_term(term: &str) -> bool {
    matches!(
        term,
        // Particles and common operation words.
        "の" | "を" | "が" | "は" | "に" | "へ" | "と" | "で" | "から" | "まで" | "も"
            | "や" | "できた" | "できました" | "終" | "わった" | "わりました" | "えた"
            | "完了" | "完了した" | "完了しました" | "済んだ" | "済み" | "した" | "して"
            | "してください" | "削除" | "消して" | "消す" | "更新" | "変更" | "直して"
            | "設定" | "セット" | "作成" | "追加" | "登録" | "にして" | "お願いします"
            // Requested attributes and dates do not identify the item.
            | "締切" | "期限" | "期日" | "日付" | "予定日"
    )
}

fn is_generic_term(term: &str) -> bool {
    matches!(
        term,
        "レポート" | "報告" | "課題" | "タスク" | "項目" | "予定" | "もの" | "やつ"
    )
}

#[cfg(test)]
mod tests {
    use super::resolve_targets;
    use crate::model::{Item, ItemKind, ItemStatus, Priority};

    fn item(title: &str, kind: ItemKind, status: ItemStatus) -> Item {
        Item {
            id: title.to_string(),
            kind,
            title: title.to_string(),
            notes: String::new(),
            status,
            project: None,
            scheduled_date: None,
            due_date: None,
            priority: Priority::None,
            tags: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
            completed_at: None,
        }
    }

    #[test]
    fn abbreviation_returns_all_matching_titles_and_preserves_item_kinds() {
        let items = vec![
            item("OSS課題レポート", ItemKind::Task, ItemStatus::Active),
            item("OSS研究レポート", ItemKind::Bute, ItemStatus::Completed),
            item("レポート", ItemKind::Task, ItemStatus::Active),
            item("別件", ItemKind::Task, ItemStatus::Active),
        ];

        let found = resolve_targets(items, "OSSのレポートできた", "OSS課題レポート").unwrap();

        assert_eq!(
            found
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec!["OSS課題レポート", "OSS研究レポート"]
        );
    }

    #[test]
    fn complete_title_in_request_returns_duplicate_titles_across_status_and_kind() {
        let items = vec![
            item("OSS課題レポート", ItemKind::Task, ItemStatus::Active),
            item("OSS課題レポート", ItemKind::Bute, ItemStatus::Completed),
            item("OSS研究レポート", ItemKind::Task, ItemStatus::Active),
        ];

        let found = resolve_targets(items, "OSS課題レポート終わった", "OSS課題レポート").unwrap();

        assert_eq!(found.len(), 2);
    }

    #[test]
    fn update_and_delete_commands_keep_the_title_words() {
        let items = vec![
            item("OSS課題レポート", ItemKind::Task, ItemStatus::Active),
            item("OSS研究レポート", ItemKind::Task, ItemStatus::Active),
        ];

        for request in [
            "OSSのレポートの締切を10/16にして",
            "OSSのレポートを削除して",
        ] {
            let found = resolve_targets(items.clone(), request, "OSS課題レポート").unwrap();
            assert_eq!(found.len(), 2, "request: {request}");
        }
    }

    #[test]
    fn unrelated_or_underspecified_references_are_rejected() {
        let items = vec![
            item("OSS課題レポート", ItemKind::Task, ItemStatus::Active),
            item("レポート", ItemKind::Task, ItemStatus::Active),
        ];

        assert!(resolve_targets(items.clone(), "それ終わった", "OSS課題レポート").is_err());
        assert_eq!(
            resolve_targets(items.clone(), "レポート終わった", "レポート")
                .unwrap()
                .len(),
            1
        );
        assert!(resolve_targets(items, "OSSのレポートできた", "無関係なタイトル").is_err());
        assert_eq!(
            resolve_targets(
                vec![item("Aufy", ItemKind::Bute, ItemStatus::Active)],
                "Aufy終わった",
                "Aufy"
            )
            .unwrap()
            .len(),
            1
        );
        assert!(resolve_targets(
            vec![item("第1回レポート", ItemKind::Task, ItemStatus::Active)],
            "第2回のレポートできた",
            "第1回レポート"
        )
        .is_err());
    }
}
