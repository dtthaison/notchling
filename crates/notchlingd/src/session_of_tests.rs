use crate::acting_ops_tests::{sessions, setup, GONE, PROMPT};
use crate::op_kinds::home;
use notchling_local_api::{ops, Handler, OpHome};
use serde_json::json;

const SESSION: &str = "6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c";

#[test]
fn session_of_finds_the_ancestor_session() {
    let s = setup(true, PROMPT, sessions());
    assert_eq!(
        s.ops.session_of(std::process::id()).as_deref(),
        Some(SESSION)
    );
    assert_eq!(s.ops.session_of(GONE).as_deref(), Some(SESSION));
}

#[test]
fn session_of_none_outside_a_session() {
    let s = setup(true, PROMPT, sessions());
    assert_eq!(s.ops.session_of(31_337), None);
    assert_eq!(s.ops.session_of(1), None);
}

#[test]
fn the_daemon_handler_resolves_a_session_through_acting() {
    let ops = crate::daemon_wiring::Ops::new(std::sync::Arc::new(std::sync::Mutex::new(json!([]))));
    assert_eq!(ops.session_of(std::process::id()), None);
    assert_eq!(ops.home(ops::SESSION_REPLY), OpHome::Kind);
    assert_eq!(home(ops::SESSION_REPLY), OpHome::Kind);
    assert!(ops.known_ops().contains(&ops::SESSION_REPLY));
}
