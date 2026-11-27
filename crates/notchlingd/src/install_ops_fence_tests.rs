use crate::install_ops_tests::{data, ops_for, rig, settings_text, Dir, USER_SETTINGS};
use notchling_local_api::ops;
use serde_json::{json, Value};

fn fence_entries(v: &Value) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for group in v["hooks"]["PreToolUse"].as_array().into_iter().flatten() {
        if group["matcher"] != "*" {
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

#[test]
fn install_plan_adds_the_fence_slot() {
    let dir = Dir::new("fence-plan");
    let (ops, _) = ops_for(rig(&dir, Some(USER_SETTINGS)), true);
    let plan = ops.run(ops::INSTALL_PLAN, &json!({})).unwrap();
    let changes = plan["plan"]["hooks"]["changes"].as_array().unwrap();
    assert!(changes
        .iter()
        .any(|c| c.as_str().unwrap().starts_with("add PreToolUse (*)")));
    assert_eq!(settings_text(&dir), USER_SETTINGS);

    let reply = ops.run(ops::INSTALL_APPLY, &json!({})).unwrap();
    assert_eq!(reply["applied"], true);
    let installed = data(&settings_text(&dir));
    let entries = fence_entries(&installed);
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0],
        (
            "'/opt/notchling/bin/notchling' hook --fence".to_string(),
            30
        )
    );
    let after = ops.run(ops::INSTALL_PLAN, &json!({})).unwrap();
    let present = after["plan"]["hooks"]["present"].as_array().unwrap();
    assert!(present
        .iter()
        .any(|p| p["matcher"] == "*" && p["event"] == "PreToolUse" && p["managed"] == true));
}

#[test]
fn remove_takes_it_out() {
    let dir = Dir::new("fence-remove");
    let (ops, _) = ops_for(rig(&dir, Some(USER_SETTINGS)), true);
    ops.run(ops::INSTALL_APPLY, &json!({})).unwrap();
    assert_eq!(fence_entries(&data(&settings_text(&dir))).len(), 1);
    let removed = ops.run(ops::INSTALL_REMOVE, &json!({})).unwrap();
    assert_eq!(removed["applied"], true);
    assert_eq!(settings_text(&dir), USER_SETTINGS);
}

#[test]
fn the_fence_slot_follows_the_prompt_hooks_and_not_the_page_hooks() {
    let dir = Dir::new("fence-what");
    let (ops, _) = ops_for(rig(&dir, Some(USER_SETTINGS)), true);
    ops.run(ops::INSTALL_APPLY, &json!({"what": ["page_hooks"]}))
        .unwrap();
    assert!(fence_entries(&data(&settings_text(&dir))).is_empty());
    ops.run(ops::INSTALL_APPLY, &json!({"what": ["prompt_hooks"]}))
        .unwrap();
    assert_eq!(fence_entries(&data(&settings_text(&dir))).len(), 1);
    ops.run(ops::INSTALL_REMOVE, &json!({"what": ["page_hooks"]}))
        .unwrap();
    assert_eq!(fence_entries(&data(&settings_text(&dir))).len(), 1);
    ops.run(ops::INSTALL_REMOVE, &json!({"what": ["prompt_hooks"]}))
        .unwrap();
    assert!(fence_entries(&data(&settings_text(&dir))).is_empty());
}
