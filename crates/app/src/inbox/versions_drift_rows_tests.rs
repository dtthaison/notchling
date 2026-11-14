use super::model::{is_local, target, InboxModel, Target};
use notchling_engine::inbox::ActionId;
use serde_json::json;

fn item() -> serde_json::Value {
    json!({
        "id": "version-drift:abc",
        "kind": "version_drift",
        "title": "vps-sing-1 Runs Another Codex",
        "detail": "vps-sing-1 runs Codex 0.167.0, the Mac 0.166.0.",
        "source": {},
        "created_ms": 1_000,
        "expires_ms": null,
        "actions": [{"id": "open", "label": "Open Machines", "primary": true}],
        "state": "open",
        "answer": null,
        "answered_by": null,
        "seen": false,
        "open": {"machines": "vps-sing-1"}
    })
}

#[test]
fn a_drift_row_names_the_machine_and_both_versions() {
    let mut model = InboxModel::default();
    model.apply(&json!({"event": "inbox.changed", "data": {"item": item()}}));
    let (needs, done) = model.sections();
    assert!(done.is_empty());
    assert_eq!(needs.len(), 1);
    let row = &needs[0];
    assert_eq!(row.title, "vps-sing-1 Runs Another Codex");
    assert_eq!(
        row.actions,
        vec![(ActionId::Open, "Open Machines".to_string(), true)]
    );
    assert!(row.enabled);
    assert_eq!(
        target(row.open.as_ref().expect("open")),
        Some(Target::Machines("vps-sing-1".to_string()))
    );
    assert!(is_local(ActionId::Open));
}

#[test]
fn a_machines_target_is_not_a_settings_page() {
    assert_eq!(
        target(&json!({"machines": "m1"})),
        Some(Target::Machines("m1".to_string()))
    );
    assert_eq!(target(&json!({"machines": 3})), None);
}
