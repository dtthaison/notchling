use super::{Capability, Group, Tool};

#[test]
fn tool_reads_agy_and_writes_antigravity() {
    assert_eq!(Tool::parse("agy"), Some(Tool::Antigravity));
    assert_eq!(
        serde_json::from_str::<Tool>("\"agy\"").unwrap(),
        Tool::Antigravity
    );
    assert_eq!(
        serde_json::to_string(&Tool::Antigravity).unwrap(),
        "\"antigravity\""
    );
    assert_eq!(Tool::parse("gemini"), None);
    assert_eq!(Tool::Antigravity.name(), "antigravity");
}

#[test]
fn row_ids_name_their_tool() {
    assert_eq!(Tool::of_row("aa11bb22"), Tool::Claude);
    assert_eq!(Tool::of_row("codex:t1"), Tool::Codex);
    assert_eq!(Tool::of_row("codex-new:1"), Tool::Codex);
    for id in ["agy:c1", "agy-new:1", "antigravity:c1", "antigravity-new:1"] {
        assert_eq!(Tool::of_row(id), Tool::Antigravity, "{id}");
    }
    assert_eq!(Tool::Claude.row_prefix(), "");
    assert_eq!(Tool::Codex.row_prefix(), "codex:");
    assert_eq!(Tool::Antigravity.row_prefix(), "antigravity:");
}

#[test]
fn thirty_nine_capabilities_with_unique_ids() {
    assert_eq!(Capability::ALL.len(), 39);
    let mut seen = std::collections::HashSet::new();
    for c in Capability::ALL {
        assert!(seen.insert(c.id()), "{} twice", c.id());
        let dots = c.id().bytes().filter(|b| *b == b'.').count();
        assert_eq!(dots, 1, "{}", c.id());
        assert!(
            c.id()
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_' || b == b'.'),
            "{}",
            c.id()
        );
        assert_eq!(Capability::parse(c.id()), Some(c));
        assert!(!c.title().is_empty() && c.title().chars().count() <= 40);
    }
    let counts: Vec<usize> = Group::ALL
        .iter()
        .map(|g| Capability::ALL.iter().filter(|c| c.group() == *g).count())
        .collect();
    assert_eq!(counts, [7, 3, 5, 3, 8, 5, 2, 3, 2, 1]);
    let order: Vec<Group> = Capability::ALL.iter().map(|c| c.group()).collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted);
}

#[test]
fn catalog_tool_is_the_contract_tool() {
    let t: crate::catalog::Tool = Tool::Codex;
    assert_eq!(t, Tool::Codex);
}
