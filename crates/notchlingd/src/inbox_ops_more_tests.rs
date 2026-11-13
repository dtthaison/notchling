use crate::acting_ops_tests::FakeRoom;
use crate::inbox_ops::InboxOps;
use crate::inbox_ops_tests::{answer, app, caller, changed, item, rig, Calls, NOW};
use crate::op_group::OpGroup;
use notchling_engine::inbox::{ActionId, State};
use notchling_local_api::{ops, ClientKind, ErrorCode, Tier};
use serde_json::json;
use std::sync::Arc;

#[test]
fn the_inbox_answers_only_the_app_itself() {
    let r = rig();
    r.ops.raise(item("a"), None);
    let cli = caller(Tier::App, ClientKind::Cli);
    for op in [ops::INBOX_LIST, ops::INBOX_ANSWER, ops::INBOX_SEEN] {
        let args = json!({"id": "a", "action": "deny", "ids": ["a"]});
        let e = r.ops.call(&cli, op, &args).unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{op}");
    }
    assert_eq!(r.ops.items()[0].state, State::Open);
    assert!(r.ops.call(&app(), ops::INBOX_LIST, &json!({})).is_ok());
}

#[test]
fn only_the_mac_or_a_phone_may_allow_a_secret() {
    let r = rig();
    r.ops.raise(item("a"), Some(Arc::new(Calls::default())));
    r.ops.raise(item("b"), Some(Arc::new(Calls::default())));
    let e = r.ops.decide("a", ActionId::Allow, "agent").unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert_eq!(r.ops.items()[1].state, State::Open);
    let denied = r.ops.decide("a", ActionId::Deny, "agent").unwrap();
    assert!(denied.answered);
    let allowed = r.ops.decide("b", ActionId::Allow, "phone").unwrap();
    assert!(allowed.answered);
    assert_eq!(allowed.answer, Some(ActionId::Allow));
}

#[test]
fn the_saved_book_loads_at_start_not_before() {
    let r = rig();
    r.ops.raise(item("a"), None);
    r.ops.flush();
    let later = NOW + 5_000;
    let room = Arc::new(FakeRoom::default());
    let reopened = InboxOps::with_clock(r.dir.clone(), room.clone(), Arc::new(move || later));
    let list = || reopened.call(&app(), ops::INBOX_LIST, &json!({})).unwrap();
    assert_eq!(list()["items"], json!([]));
    let mut b = item("b");
    b.created_ms = NOW + 1;
    reopened.raise(b, None);
    reopened.start();
    let ids: Vec<String> = reopened.items().into_iter().map(|i| i.id).collect();
    assert_eq!(ids, ["b", "a"]);
    assert_eq!(list()["items"][0]["state"], "open");
    assert_eq!(list()["items"][1]["state"], "withdrawn");
    reopened.start();
    assert_eq!(reopened.items().len(), 2);
    reopened.close();
    let saved: Vec<String> = changed(&room)
        .iter()
        .map(|e| e["item"]["id"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(saved.contains(&"a".to_string()));
}

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
