use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_ops::Opening;
use crate::consent_testkit::{agent_caller, consent};
use crate::inbox_ops::InboxOps;
use crate::limits_ops_tests::TempDir;
use notchling_engine::consent::{COST_PROFILE, INSTALL, MACHINE, MODEL, UPDATE};
use notchling_events::field::FieldValue;
use notchling_events::MemorySink;
use serde_json::json;
use std::sync::Arc;

fn row_of(tag: &str) -> Consent {
    let tmp = TempDir::new(&format!("consent-events-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let ops = consent(room, inbox, Arc::new(MemorySink::new()));
    let opened = ops
        .open(
            &agent_caller(),
            Opening {
                parts: None,
                kind: MACHINE,
                target: "t".to_string(),
                change: json!({"action": "x", "apply": {"op": "x", "args": {}}}),
                facts: json!([]),
                request: json!({"op": "x"}),
                reason: None,
                expires_in_ms: None,
                key: None,
            },
        )
        .unwrap();
    let row = opened.row().clone();
    ops.close();
    row
}

fn action_of(row: &Consent) -> Option<&'static str> {
    match event_of(row, ConsentThen::Asked, 0).fields.get("action")? {
        FieldValue::Name(name) => Some(name),
        _ => None,
    }
}

fn with(row: &Consent, kind: &str, action: Option<&str>) -> Consent {
    let mut row = row.clone();
    row.kind = kind.to_string();
    row.change = match action {
        Some(action) => json!({"action": action}),
        None => json!({}),
    };
    row
}

#[test]
fn an_update_names_the_tool_update_action() {
    let base = row_of("update");
    for action in [None, Some("update"), Some("back")] {
        assert_eq!(
            action_of(&with(&base, UPDATE, action)),
            Some("tool_update"),
            "{action:?}"
        );
    }
}

#[test]
fn a_model_download_names_its_action() {
    let base = row_of("model");
    assert_eq!(action_of(&with(&base, MODEL, None)), Some("model_download"));
}

#[test]
fn a_set_up_names_the_install_action() {
    let base = row_of("install");
    assert_eq!(
        action_of(&with(&base, INSTALL, Some("install"))),
        Some("install_set_up")
    );
    assert_eq!(
        action_of(&with(&base, INSTALL, None)),
        Some("install_set_up")
    );
}

#[test]
fn a_cost_profile_request_names_set_apply_or_undo_by_its_action() {
    let base = row_of("cost");
    for (action, want) in [
        ("profile", Some("cost_set")),
        ("apply", Some("cost_apply")),
        ("undo", Some("cost_undo")),
        ("sideways", None),
    ] {
        assert_eq!(
            action_of(&with(&base, COST_PROFILE, Some(action))),
            want,
            "{action}"
        );
    }
    assert_eq!(action_of(&with(&base, COST_PROFILE, None)), None);
}

#[test]
fn a_machine_request_names_its_action_by_what_the_card_changes() {
    let base = row_of("machine");
    for (action, want) in [
        ("add", Some("machine_add")),
        ("import", Some("machine_add")),
        ("remove", Some("machine_remove")),
        ("fix", Some("machine_fix")),
        ("setup", Some("machine_fix")),
        ("running", Some("machine_fix")),
        ("addresses", Some("machine_update")),
        ("update", Some("machine_update")),
        ("network", Some("tailscale_network")),
        ("rename", None),
        ("sideways", None),
    ] {
        assert_eq!(
            action_of(&with(&base, MACHINE, Some(action))),
            want,
            "{action}"
        );
    }
    assert_eq!(action_of(&with(&base, MACHINE, None)), None);
}

fn names_a_machine(row: &Consent) -> bool {
    event_of(row, ConsentThen::Asked, 0)
        .fields
        .get("machine")
        .is_some_and(|v| *v != FieldValue::Null)
}

#[test]
fn a_machine_card_names_its_machine_and_no_other_kind_does() {
    let base = row_of("machine-ref");
    let mut named = with(&base, MACHINE, Some("fix"));
    named.change["machine"] = json!("m1");
    assert!(names_a_machine(&named));
    let plain = with(&base, MACHINE, Some("network"));
    assert!(!names_a_machine(&plain));
    let mut other = with(&base, COST_PROFILE, Some("apply"));
    other.change["machine"] = json!("m1");
    assert!(!names_a_machine(&other));
}

#[test]
fn the_parts_of_a_card_are_counted_never_named() {
    let mut base = row_of("count");
    base.parts = Some(json!([{"id": "a"}, {"id": "b"}, {"id": "c"}]));
    base.parts_allowed = Some(json!(["a"]));
    let event = event_of(&base, ConsentThen::Asked, 0);
    assert_eq!(event.fields.get("parts"), Some(&FieldValue::UInt(3)));
    assert_eq!(
        event.fields.get("parts_allowed"),
        Some(&FieldValue::UInt(1))
    );
}
