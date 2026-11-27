use super::model::{is_local, target, InboxModel, Target};
use notchling_engine::inbox::ActionId;
use serde_json::{json, Value};

fn item(state: &str) -> Value {
    json!({
        "id": "version-check:abc",
        "kind": "version_check_failed",
        "title": "Claude Code 2.1.324 Didn't Pass",
        "detail": "`claude doctor` no longer prints its auto-update line. Workers stay on 2.1.323.",
        "source": {},
        "created_ms": 1_000,
        "expires_ms": null,
        "actions": [{"id": "open", "label": "Open Settings", "primary": true}],
        "state": state,
        "answer": null,
        "answered_by": null,
        "seen": false,
        "open": {"settings": "agents", "tool": "claude"}
    })
}

#[test]
fn row_opens_settings_agents() {
    let mut model = InboxModel::default();
    model.apply(&json!({"event": "inbox.changed", "data": {"item": item("open")}}));
    let (needs, done) = model.sections();
    assert!(done.is_empty());
    assert_eq!(needs.len(), 1);
    let row = &needs[0];
    assert_eq!(row.title, "Claude Code 2.1.324 Didn't Pass");
    assert_eq!(
        row.actions,
        vec![(ActionId::Open, "Open Settings".to_string(), true)]
    );
    assert!(row.enabled);
    let open = row.open.as_ref().expect("open");
    assert_eq!(
        target(open),
        Some(Target::Settings {
            page: "agents".to_string(),
            tool: Some("claude".to_string())
        })
    );
    assert!(is_local(ActionId::Open));
}

#[test]
fn a_withdrawn_notice_is_done_and_says_so() {
    let mut model = InboxModel::default();
    model.apply(&json!({"event": "inbox.changed", "data": {"item": item("withdrawn")}}));
    let (needs, done) = model.sections();
    assert!(needs.is_empty());
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].result.as_deref(), Some("Withdrawn"));
    assert!(!done[0].enabled);
}

#[test]
fn vault_targets_still_win_over_settings() {
    let both = json!({"vault_approval": "t1", "settings": "agents"});
    assert_eq!(target(&both), Some(Target::Approval("t1".to_string())));
    assert_eq!(target(&json!({"settings": 3})), None);
}
