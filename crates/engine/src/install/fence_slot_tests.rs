use super::command::{classify, Origin};
use super::hooks::*;
use super::testutil::*;
use serde_json::Value;

const FENCE_CMD: &str = "'/opt/notchling/bin/notchling' hook --fence";

fn fence_entries(v: &Value) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for group in v["hooks"]["PreToolUse"].as_array().into_iter().flatten() {
        if group["matcher"].as_str() != Some("*") {
            continue;
        }
        for h in group["hooks"].as_array().into_iter().flatten() {
            out.push((
                h["command"].as_str().unwrap_or("").to_string(),
                h["timeout"].as_u64().unwrap_or(0),
            ));
        }
    }
    out
}

fn only_fence(want: Want) -> Wanted {
    Wanted {
        command: CMD.to_string(),
        prompt: Want::Leave,
        pages: Want::Leave,
        fence: want,
    }
}

#[test]
fn install_plan_adds_the_fence_slot() {
    let out = plan_hooks(NO_HOOKS, &only_fence(Want::Install)).unwrap();
    assert!(out.changed);
    assert_eq!(out.changes.len(), 1, "{:?}", out.changes);
    assert!(
        out.changes[0].starts_with("add PreToolUse (*)"),
        "{:?}",
        out.changes
    );
    assert_eq!(
        fence_entries(&value(&out.text)),
        vec![(FENCE_CMD.to_string(), 30)]
    );
    assert_eq!(classify(FENCE_CMD), Some(Origin::Notchling));
    let found = detect(&out.text).unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].managed);
    assert_eq!(found[0].event, "PreToolUse");
    assert_eq!(found[0].matcher.as_deref(), Some("*"));
    let again = plan_hooks(&out.text, &only_fence(Want::Install)).unwrap();
    assert!(!again.changed);
    assert!(again.notes.iter().any(|n| n.ends_with("already installed")));
}

#[test]
fn remove_takes_it_out() {
    for text in [NO_HOOKS, USER_HOOKS, EMPTY_HOOKS, COMPACT] {
        let installed = plan_hooks(text, &only_fence(Want::Install)).unwrap();
        let back = plan_hooks(&installed.text, &only_fence(Want::Remove)).unwrap();
        assert!(back.changed);
        assert_eq!(back.changes.len(), 1, "{:?}", back.changes);
        assert!(fence_entries(&value(&back.text)).is_empty());
        let none = plan_hooks(&back.text, &only_fence(Want::Remove)).unwrap();
        assert!(!none.changed);
    }
}

#[test]
fn the_fence_slot_leaves_the_other_five_alone() {
    let all = plan_hooks(NO_HOOKS, &install_all()).unwrap();
    assert_eq!(all.changes.len(), 6, "{:?}", all.changes);
    let without = plan_hooks(&all.text, &only_fence(Want::Remove)).unwrap();
    assert_eq!(without.changes.len(), 1);
    let five = plan_hooks(NO_HOOKS, &wanted(Want::Install, Want::Install)).unwrap();
    assert_eq!(five.changes.len(), 5);
    assert_eq!(value(&without.text), value(&five.text));
    let leave = plan_hooks(&all.text, &wanted(Want::Remove, Want::Remove)).unwrap();
    assert_eq!(fence_entries(&value(&leave.text)).len(), 1);
}

#[test]
fn a_stale_fence_entry_is_replaced() {
    let stale = r#"{"hooks": {"PreToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "'/old/notchling' hook --fence", "timeout": 5}]}]}}"#;
    let out = plan_hooks(stale, &only_fence(Want::Install)).unwrap();
    assert!(out.changed);
    assert_eq!(
        fence_entries(&value(&out.text)),
        vec![(FENCE_CMD.to_string(), 30)]
    );
    assert!(out
        .changes
        .iter()
        .any(|c| c.starts_with("replace PreToolUse (*)")));
}

#[test]
fn a_users_own_pre_tool_use_star_hook_is_never_touched() {
    let mine = r#"{"hooks": {"PreToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "~/bin/audit.sh"}]}]}}"#;
    let out = plan_hooks(mine, &only_fence(Want::Install)).unwrap();
    let entries = fence_entries(&value(&out.text));
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|e| e.0 == "~/bin/audit.sh"));
    let back = plan_hooks(&out.text, &only_fence(Want::Remove)).unwrap();
    assert_eq!(value(&back.text), value(mine));
}
