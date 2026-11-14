use super::mismatch::mismatch;
use super::worker::{ToolState, WorkerTools};
use super::{Method, Tool, Version};
use std::collections::BTreeMap;

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn installed(version: Option<&str>) -> ToolState {
    ToolState::Installed {
        version: version.map(v),
        method: Method::Native,
        path: "/h/bin/x".into(),
    }
}

fn worker(entries: &[(Tool, ToolState)]) -> WorkerTools {
    WorkerTools {
        tools: entries.iter().cloned().collect(),
        at_ms: 0,
    }
}

fn mac(entries: &[(Tool, &str)]) -> BTreeMap<Tool, Version> {
    entries.iter().map(|(t, s)| (*t, v(s))).collect()
}

#[test]
fn names_both_versions() {
    let found = mismatch(
        &mac(&[(Tool::Claude, "2.1.324")]),
        "m1",
        &worker(&[(Tool::Claude, installed(Some("2.1.323")))]),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].text(),
        "m1 runs Claude Code 2.1.323, the Mac 2.1.324"
    );
}

#[test]
fn absent_tool_is_never_a_mismatch() {
    let found = mismatch(
        &mac(&[(Tool::Claude, "2.1.324"), (Tool::Codex, "0.166.0")]),
        "m1",
        &worker(&[(Tool::Claude, ToolState::NotInstalled)]),
    );
    assert!(found.is_empty());
}

#[test]
fn unknown_version_is_left_out() {
    let found = mismatch(
        &mac(&[(Tool::Claude, "2.1.324")]),
        "m1",
        &worker(&[(Tool::Claude, installed(None))]),
    );
    assert!(found.is_empty());
    let found = mismatch(
        &mac(&[]),
        "m1",
        &worker(&[(Tool::Claude, installed(Some("2.1.1")))]),
    );
    assert!(found.is_empty());
}

#[test]
fn equal_versions_are_no_mismatch() {
    let found = mismatch(
        &mac(&[(Tool::Claude, "2.1.324")]),
        "m1",
        &worker(&[(Tool::Claude, installed(Some("2.1.324")))]),
    );
    assert!(found.is_empty());
}

#[test]
fn several_tools_come_in_tool_order() {
    let found = mismatch(
        &mac(&[(Tool::Codex, "0.166.0"), (Tool::Claude, "2.1.324")]),
        "vps-1",
        &worker(&[
            (Tool::Codex, installed(Some("0.165.0"))),
            (Tool::Claude, installed(Some("2.1.325"))),
        ]),
    );
    let tools: Vec<Tool> = found.iter().map(|m| m.tool).collect();
    assert_eq!(tools, vec![Tool::Claude, Tool::Codex]);
    assert_eq!(found[1].text(), "vps-1 runs Codex 0.165.0, the Mac 0.166.0");
}
