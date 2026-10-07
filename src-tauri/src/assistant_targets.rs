use crate::model::Item;

pub(crate) struct TargetResolution {
    pub items: Vec<Item>,
    pub needs_confirmation: bool,
}

/// The model extracts a literal name reference; do not parse Japanese operations here.
/// Return every matching candidate, even when the model supplied one canonical title.
pub(crate) fn resolve_targets(
    items: Vec<Item>,
    reference: &str,
    tool_title: &str,
) -> Result<TargetResolution, String> {
    let reference = normalize(reference);
    let tool_title = normalize(tool_title);
    if reference.is_empty() || tool_title.is_empty() || items.is_empty() {
        return Err(target_error());
    }
    let matches: Vec<_> = items
        .iter()
        .filter(|item| contains_name(&normalize(&item.title), &reference))
        .cloned()
        .collect();
    if !matches.is_empty() {
        let needs_confirmation = tool_title != reference
            && !matches
                .iter()
                .any(|item| normalize(&item.title) == tool_title);
        return Ok(TargetResolution {
            items: matches,
            needs_confirmation,
        });
    }
    // A model title is only a suggestion when its literal reference did not
    // resolve. Never execute against a suggested target without user selection.
    let suggested: Vec<_> = items
        .iter()
        .filter(|item| {
            let title = normalize(&item.title);
            contains_name(&title, &tool_title) || contains_name(&tool_title, &title)
        })
        .cloned()
        .collect();
    Ok(TargetResolution {
        items: if suggested.is_empty() {
            items
        } else {
            suggested
        },
        needs_confirmation: true,
    })
}

fn target_error() -> String {
    "対象を特定できませんでした。項目のタイトルを明示してください。".into()
}

fn normalize(value: &str) -> String {
    value
        .trim()
        .trim_matches(['「', '」', '『', '』', '\"', '\''])
        .to_lowercase()
}

fn contains_name(title: &str, reference: &str) -> bool {
    title.match_indices(reference).any(|(start, _)| {
        let end = start + reference.len();
        // Keep ASCII names whole: Aufy must not silently match AufyNext.
        let attached_before = reference
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
            && title[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric());
        let attached_after = reference
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric())
            && title[end..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric());
        !attached_before && !attached_after
    })
}

#[cfg(test)]
mod tests {
    use super::resolve_targets;
    use crate::model::{Item, ItemKind, ItemStatus, Priority};

    fn item(title: &str, kind: ItemKind, status: ItemStatus) -> Item {
        Item {
            id: title.into(),
            title: title.into(),
            kind,
            status,
            notes: String::new(),
            project: None,
            scheduled_date: None,
            due_date: None,
            priority: Priority::None,
            tags: vec![],
            created_at: String::new(),
            updated_at: String::new(),
            completed_at: None,
        }
    }

    #[test]
    fn extracted_reference_returns_all_candidates_without_model_narrowing() {
        let items = vec![
            item("Aufyの開発", ItemKind::Bute, ItemStatus::Active),
            item("Aufyの検証", ItemKind::Task, ItemStatus::Completed),
            item("AufyNextの開発", ItemKind::Bute, ItemStatus::Active),
        ];
        for hint in ["Aufy", "Aufyの開発"] {
            let matches = resolve_targets(items.clone(), "Aufy", hint).unwrap();
            assert!(!matches.needs_confirmation);
            assert_eq!(matches.items.len(), 2);
            assert_eq!(matches.items[0].title, "Aufyの開発");
            assert_eq!(matches.items[1].title, "Aufyの検証");
        }
    }

    #[test]
    fn duplicate_complete_titles_are_kept_across_kind_and_status() {
        let items = vec![
            item("課題", ItemKind::Task, ItemStatus::Active),
            item("課題", ItemKind::Bute, ItemStatus::Completed),
        ];
        assert_eq!(
            resolve_targets(items, "課題", "課題").unwrap().items.len(),
            2
        );
    }

    #[test]
    fn uncertain_references_and_inconsistent_titles_require_user_selection() {
        let items = vec![item("Aufyの開発", ItemKind::Bute, ItemStatus::Active)];
        for (reference, hint) in [
            ("Aufy", "別の項目"),
            ("それ", "Aufyの開発"),
            ("Aufyのプロジェクト", "Aufyの開発"),
        ] {
            let targets = resolve_targets(items.clone(), reference, hint).unwrap();
            assert!(targets.needs_confirmation);
            assert_eq!(targets.items[0].title, "Aufyの開発");
        }
        let exact = resolve_targets(items, "aufy", "Aufyの開発").unwrap();
        assert!(!exact.needs_confirmation);
        assert_eq!(exact.items.len(), 1);
        let targets = resolve_targets(
            vec![item("AufyNext", ItemKind::Bute, ItemStatus::Active)],
            "Aufy",
            "AufyNext",
        )
        .unwrap();
        assert!(targets.needs_confirmation);

        let all = vec![
            item("レポート", ItemKind::Task, ItemStatus::Active),
            item("開発", ItemKind::Bute, ItemStatus::Active),
        ];
        let targets = resolve_targets(all, "見つからない名前", "見つからない項目").unwrap();
        assert!(targets.needs_confirmation);
        assert_eq!(targets.items.len(), 2);
        assert!(resolve_targets(vec![], "名前", "項目").is_err());
    }
}
