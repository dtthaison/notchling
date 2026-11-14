use super::drift::{drift, plan_notices, Drift, Key};
use super::worker::{ToolState, WorkerTools};
use super::{Method, Tool, Version};
use std::collections::{BTreeMap, BTreeSet};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn have(version: Option<&str>) -> ToolState {
    ToolState::Installed {
        version: version.map(v),
        method: Method::Native,
        path: "/h/bin/x".into(),
    }
}

fn workers(entries: &[(&str, &[(Tool, ToolState)])]) -> BTreeMap<String, WorkerTools> {
    entries
        .iter()
        .map(|(name, tools)| {
            (
                name.to_string(),
                WorkerTools {
                    tools: tools.iter().cloned().collect(),
                    at_ms: 0,
                },
            )
        })
        .collect()
}

fn reference(entries: &[(Tool, &str)]) -> BTreeMap<Tool, Version> {
    entries.iter().map(|(t, s)| (*t, v(s))).collect()
}

fn none() -> BTreeSet<(String, Tool)> {
    BTreeSet::new()
}

#[test]
fn behind_and_ahead_are_drift() {
    let found = drift(
        &reference(&[(Tool::Codex, "0.166.0")]),
        &workers(&[
            ("a", &[(Tool::Codex, have(Some("0.167.0")))]),
            ("b", &[(Tool::Codex, have(Some("0.165.0")))]),
            ("c", &[(Tool::Codex, have(Some("0.166.0")))]),
        ]),
        &none(),
        &none(),
    );
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].machine, "a");
    assert!(found[0].ahead);
    assert_eq!(found[1].machine, "b");
    assert!(!found[1].ahead);
    assert_eq!(found[1].expected, v("0.166.0"));
    assert_eq!(found[1].found, v("0.165.0"));
}

#[test]
fn absent_tool_is_never_drift() {
    let found = drift(
        &reference(&[(Tool::Codex, "0.166.0"), (Tool::Claude, "2.1.324")]),
        &workers(&[(
            "a",
            &[
                (Tool::Codex, ToolState::NotInstalled),
                (Tool::Claude, have(None)),
            ],
        )]),
        &none(),
        &none(),
    );
    assert!(found.is_empty());
}

#[test]
fn held_is_not_drift() {
    let held: BTreeSet<(String, Tool)> = [("a".to_string(), Tool::Codex)].into();
    let rolling: BTreeSet<(String, Tool)> = [("b".to_string(), Tool::Codex)].into();
    let found = drift(
        &reference(&[(Tool::Codex, "0.166.0")]),
        &workers(&[
            ("a", &[(Tool::Codex, have(Some("0.167.0")))]),
            ("b", &[(Tool::Codex, have(Some("0.165.0")))]),
            ("c", &[(Tool::Codex, have(Some("0.165.0")))]),
        ]),
        &held,
        &rolling,
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].machine, "c");
}

#[test]
fn no_usable_reference_is_no_drift() {
    let found = drift(
        &reference(&[(Tool::Claude, "2.1.324")]),
        &workers(&[("a", &[(Tool::Codex, have(Some("0.167.0")))])]),
        &none(),
        &none(),
    );
    assert!(found.is_empty());
    assert!(drift(&BTreeMap::new(), &BTreeMap::new(), &none(), &none()).is_empty());
}

fn key(machine: &str, tool: Tool, found: &str) -> Key {
    (machine.to_string(), tool, found.to_string())
}

fn one(found: &str) -> Vec<Drift> {
    drift(
        &reference(&[(Tool::Codex, "0.166.0")]),
        &workers(&[("vps", &[(Tool::Codex, have(Some(found)))])]),
        &none(),
        &none(),
    )
}

#[test]
fn one_notice_per_found_version() {
    let first = plan_notices(&one("0.167.0"), &BTreeSet::new());
    assert_eq!(first.raise.len(), 1);
    assert!(first.withdraw.is_empty());
    assert_eq!(first.noticed, [key("vps", Tool::Codex, "0.167.0")].into());

    let again = plan_notices(&one("0.167.0"), &first.noticed);
    assert!(again.raise.is_empty());
    assert!(again.withdraw.is_empty());

    let moved = plan_notices(&one("0.168.0"), &first.noticed);
    assert_eq!(moved.raise.len(), 1);
    assert_eq!(moved.withdraw, vec![key("vps", Tool::Codex, "0.167.0")]);
    assert_eq!(moved.noticed, [key("vps", Tool::Codex, "0.168.0")].into());
}

#[test]
fn matching_again_withdraws_the_notice() {
    let noticed: BTreeSet<Key> = [key("vps", Tool::Codex, "0.167.0")].into();
    let plan = plan_notices(&[], &noticed);
    assert!(plan.raise.is_empty());
    assert_eq!(plan.withdraw, vec![key("vps", Tool::Codex, "0.167.0")]);
    assert!(plan.noticed.is_empty());

    let back = plan_notices(&one("0.167.0"), &plan.noticed);
    assert_eq!(back.raise.len(), 1);
}

#[test]
fn notice_text_names_both_versions() {
    let found = one("0.167.0");
    assert_eq!(found[0].title(), "vps Runs Another Codex");
    assert_eq!(
        found[0].detail(),
        "vps runs Codex 0.167.0, the Mac 0.166.0."
    );
}
