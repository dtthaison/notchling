use std::collections::BTreeMap;

use super::{mismatch_between, Tool, Version};

fn tools(items: &[(Tool, &str)]) -> BTreeMap<Tool, Version> {
    items
        .iter()
        .map(|(t, v)| (*t, Version::parse(v).expect("a version")))
        .collect()
}

#[test]
fn names_both_versions() {
    let mac = tools(&[(Tool::Claude, "2.1.324")]);
    let worker = tools(&[(Tool::Claude, "2.1.323")]);
    let found = mismatch_between(&mac, "m1", &worker);
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].text(),
        "m1 runs Claude Code 2.1.323, the Mac 2.1.324"
    );
}

#[test]
fn absent_tool_is_never_a_mismatch() {
    let mac = tools(&[(Tool::Claude, "2.1.324"), (Tool::Codex, "0.166.0")]);
    let worker = tools(&[(Tool::Codex, "0.166.0")]);
    assert!(mismatch_between(&mac, "m1", &worker).is_empty());
}

#[test]
fn equal_versions_are_no_mismatch() {
    let both = tools(&[(Tool::Claude, "2.1.324")]);
    assert!(mismatch_between(&both, "m1", &both).is_empty());
}
