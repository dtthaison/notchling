use crate::inbox_ops_tests::{answer, changed, item, rig, Calls, NOW};
use notchling_engine::inbox::{ActionId, State};
use std::sync::Arc;

#[test]
fn tick_never_expires_an_item_whose_producer_still_waits() {
    let r = rig();
    let calls = Arc::new(Calls::default());
    let mut held = item("held");
    held.expires_ms = Some(NOW + 1_000);
    r.ops.raise(held, Some(calls.clone()));
    let mut loose = item("loose");
    loose.expires_ms = Some(NOW + 1_000);
    r.ops.raise(loose, None);
    r.ops.tick(NOW + 60_000);
    r.ops.flush();
    let state = |id: &str| {
        r.ops
            .items()
            .into_iter()
            .find(|i| i.id == id)
            .map(|i| i.state)
    };
    assert_eq!(state("held"), Some(State::Open));
    assert_eq!(state("loose"), Some(State::Expired));
    let out = answer(&r.ops, "held", "deny");
    assert_eq!(out["answered"], true);
    assert_eq!(state("held"), Some(State::Answered));
    assert_eq!(
        calls.0.lock().unwrap().clone(),
        [("held".to_string(), ActionId::Deny)]
    );
}

#[test]
fn the_producer_expires_its_own_item() {
    let r = rig();
    let mut held = item("held");
    held.expires_ms = Some(NOW + 1_000);
    r.ops.raise(held, Some(Arc::new(Calls::default())));
    r.ops.tick(NOW + 60_000);
    r.ops.resolve("held", State::Expired, None, None);
    r.ops.flush();
    let last = changed(&r.room).last().cloned().unwrap();
    assert_eq!(last["item"]["state"], "expired");
}

#[test]
fn a_producer_answer_records_who_answered() {
    let r = rig();
    r.ops.raise(item("a"), Some(Arc::new(Calls::default())));
    r.ops.resolve(
        "a",
        State::Answered,
        Some(ActionId::AllowAlways),
        Some("mac"),
    );
    r.ops.flush();
    let last = changed(&r.room).last().cloned().unwrap();
    assert_eq!(last["item"]["answer"], "allow_always");
    assert_eq!(last["item"]["answered_by"], "mac");
}
