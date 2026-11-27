use crate::providers_add_tests::{adds_of, ends, lone_rig};
use crate::providers_ops::{ProvidersOps, ACCOUNTS_OFF};
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

fn cancel_now(ops: &Arc<ProvidersOps>, args: Value) -> Box<dyn FnOnce() + Send> {
    let canceller = ops.clone();
    Box::new(move || {
        canceller.set_reply_wait(Duration::from_millis(20));
        canceller.call(ops::PROVIDERS_ADD_CANCEL, &args).unwrap();
    })
}

#[test]
fn a_cancel_asked_while_an_add_reads_the_provider_stops_it_before_it_begins() {
    let (ops, room, one, _) = lone_rig();
    *one.on_sync.lock().unwrap() = Some(cancel_now(&ops, json!({"provider": "one"})));

    let refused = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap_err();

    assert_eq!(refused.message, "The sign-in was cancelled.");
    assert_eq!(ends(&room, 1, "cancelled"), ["cancelled"]);
    assert_eq!(*one.began.lock().unwrap(), 0, "the add never began");
}

#[test]
fn accounts_turned_off_while_an_add_begins_well_refuse_it_and_abandon_it() {
    let (ops, room, one, flag) = lone_rig();
    *one.on_begin.lock().unwrap() = Some(Box::new(move || {
        flag.set(false);
    }));

    let refused = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap_err();

    assert_eq!(refused.code, ErrorCode::Forbidden);
    assert_eq!(refused.message, ACCOUNTS_OFF);
    assert_eq!(ends(&room, 1, "cancelled"), ["cancelled"]);
    assert_eq!(*one.abandons.lock().unwrap(), 1);
    assert_eq!(
        *one.cancels.lock().unwrap(),
        0,
        "no login touched while off"
    );
}

#[test]
fn stopping_with_accounts_off_while_an_add_begins_still_abandons_it() {
    let (ops, room, one, flag) = lone_rig();
    let closer = ops.clone();
    *one.on_begin.lock().unwrap() = Some(Box::new(move || {
        flag.set(false);
        closer.close();
    }));

    let refused = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap_err();

    assert_eq!(refused.message, "Notchling is stopping.");
    assert_eq!(ends(&room, 1, "cancelled"), ["cancelled"]);
    assert_eq!(*one.abandons.lock().unwrap(), 1);
    assert_eq!(
        *one.cancels.lock().unwrap(),
        0,
        "no login touched while off"
    );
}

#[test]
fn stopping_while_an_add_fails_to_begin_refuses_it_as_stopping() {
    let (ops, room, one, _) = lone_rig();
    *one.refuses.lock().unwrap() = true;
    let closer = ops.clone();
    *one.on_begin.lock().unwrap() = Some(Box::new(move || closer.close()));

    let refused = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap_err();

    assert_eq!(refused.message, "Notchling is stopping.");
    assert_eq!(ends(&room, 1, "cancelled"), ["cancelled"]);
    assert_eq!(adds_of(&room, 1)[0]["error"], "Notchling is stopping.");
}

#[test]
fn an_add_asked_after_stopping_is_refused_before_it_asks_to_confirm() {
    let (ops, room, one, _) = lone_rig();
    *one.confirms.lock().unwrap() = true;
    ops.close();

    let refused = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap_err();

    assert_eq!(refused.message, "Notchling is stopping.");
    assert_eq!(ends(&room, 1, "cancelled"), ["cancelled"]);
}

#[test]
fn a_cancel_asked_while_an_add_begins_refuses_it_with_one_cancelled_event() {
    let (ops, room, one, _) = lone_rig();
    *one.on_begin.lock().unwrap() = Some(cancel_now(&ops, json!({"provider": "one"})));

    let refused = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap_err();

    assert_eq!(refused.message, "The sign-in was cancelled.");
    assert_eq!(ends(&room, 1, "cancelled"), ["cancelled"]);
    assert_eq!(*one.cancels.lock().unwrap(), 1, "ended once, by the add");
}

#[test]
fn a_cancel_asked_while_an_add_begins_never_touches_logins_once_accounts_are_off() {
    let (ops, room, one, flag) = lone_rig();
    let cancel = cancel_now(&ops, json!({"provider": "one"}));
    *one.on_begin.lock().unwrap() = Some(Box::new(move || {
        cancel();
        flag.set(false);
    }));

    let refused = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap_err();

    assert_eq!(refused.message, ACCOUNTS_OFF);
    assert_eq!(ends(&room, 1, "cancelled"), ["cancelled"]);
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        *one.cancels.lock().unwrap(),
        0,
        "no login touched while off"
    );
    assert_eq!(*one.abandons.lock().unwrap(), 1);
}

#[test]
fn an_add_halted_right_after_it_began_says_where_it_left_things() {
    let (ops, room, one, flag) = lone_rig();
    *one.on_begin.lock().unwrap() = Some(Box::new(move || {
        flag.set(false);
    }));

    ops.call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap_err();

    ends(&room, 1, "cancelled");
    assert_eq!(adds_of(&room, 1)[0]["note"], "Left signed out.");
}

#[test]
fn a_cancel_for_another_attempt_leaves_the_running_add_alone() {
    let (ops, room, one, _) = lone_rig();
    ops.call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap();

    let stale = ops
        .call(
            ops::PROVIDERS_ADD_CANCEL,
            &json!({"provider": "one", "attempt": 7}),
        )
        .unwrap();
    assert_eq!(stale, json!({"cancelled": true}));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(*one.cancels.lock().unwrap(), 0);
    assert_eq!(*one.abandons.lock().unwrap(), 0);
    assert!(!adds_of(&room, 1).iter().any(|e| e["state"] == "cancelled"));

    let asked = ops
        .call(
            ops::PROVIDERS_ADD_CANCEL,
            &json!({"provider": "one", "attempt": 1}),
        )
        .unwrap();
    assert_eq!(asked, json!({"cancelled": true}));
    assert_eq!(ends(&room, 1, "cancelled"), ["started", "cancelled"]);
    assert_eq!(*one.cancels.lock().unwrap(), 1);
}

fn running(ops: &ProvidersOps) -> Value {
    let list = ops
        .call(ops::PROVIDERS_LIST, &json!({"sync": false}))
        .unwrap();
    list["providers"][0]["state"]["add_attempt"].clone()
}

fn listed(ops: &ProvidersOps) -> Value {
    let list = ops
        .call(ops::PROVIDERS_LIST, &json!({"sync": false}))
        .unwrap();
    list["providers"][0]["state"].clone()
}

fn held() -> (std::sync::mpsc::Sender<()>, Box<dyn FnOnce() + Send>) {
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let hook = Box::new(move || {
        let _ = wait.recv_timeout(Duration::from_secs(10));
    });
    (go, hook)
}

#[test]
fn the_list_names_an_add_from_a_slow_begin_until_a_slow_cancel_has_ended_it() {
    let (ops, room, one, _) = lone_rig();
    assert_eq!(running(&ops), Value::Null);
    let (begun, hold) = held();
    *one.on_begin.lock().unwrap() = Some(hold);
    ops.set_reply_wait(Duration::from_millis(20));
    let reply = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap();
    assert_eq!(reply["state"], "starting");
    assert_eq!(running(&ops), reply["attempt"], "a slow begin");
    begun.send(()).unwrap();
    ends(&room, 1, "started");

    let (ended, hold) = held();
    *one.on_cancel.lock().unwrap() = Some(hold);
    let asked = json!({"provider": "one", "attempt": 1});
    let cancel = ops.call(ops::PROVIDERS_ADD_CANCEL, &asked).unwrap();
    assert_eq!(cancel, json!({"state": "cancelling"}));
    assert_eq!(running(&ops), reply["attempt"], "a slow cancel");
    ended.send(()).unwrap();
    ops.set_reply_wait(Duration::from_secs(5));
    let cancel = ops.call(ops::PROVIDERS_ADD_CANCEL, &asked).unwrap();
    assert_eq!(cancel, json!({"cancelled": true}));
    assert_eq!(running(&ops), Value::Null, "ended");
}

#[test]
fn a_provider_reads_busy_from_a_slow_begin_until_its_add_has_ended() {
    let (ops, room, one, _) = lone_rig();
    assert_eq!(listed(&ops)["busy"], false);
    let (begun, hold) = held();
    *one.on_begin.lock().unwrap() = Some(hold);
    ops.set_reply_wait(Duration::from_millis(20));
    let begin = json!({"provider": "one"});
    let reply = ops.call(ops::PROVIDERS_ADD_BEGIN, &begin).unwrap();
    assert_eq!(reply["state"], "starting");
    let state = listed(&ops);
    assert_eq!(state["busy"], true, "a slow begin: {state}");
    assert_eq!(state["adding"], false);

    let second = ops.call(ops::PROVIDERS_ADD_BEGIN, &begin).unwrap_err();
    assert!(second.message.contains("another account change"));
    assert_eq!(running(&ops), reply["attempt"], "the first add still runs");
    assert_eq!(*one.began.lock().unwrap(), 1);

    begun.send(()).unwrap();
    ends(&room, 1, "started");
    ops.set_reply_wait(Duration::from_secs(5));
    let asked = json!({"provider": "one", "attempt": 1});
    let cancel = ops.call(ops::PROVIDERS_ADD_CANCEL, &asked).unwrap();
    assert_eq!(cancel, json!({"cancelled": true}));
    let state = listed(&ops);
    assert_eq!(state["busy"], false, "ended: {state}");
    assert_eq!(state.get("add_attempt"), None);
}

#[test]
fn the_list_while_accounts_are_off_names_no_add() {
    let (ops, room, one, flag) = lone_rig();
    let (begun, hold) = held();
    *one.on_begin.lock().unwrap() = Some(hold);
    ops.set_reply_wait(Duration::from_millis(20));
    let reply = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap();
    assert_eq!(reply["state"], "starting");
    flag.set(false);
    let off = ops.call(ops::PROVIDERS_LIST, &json!({})).unwrap();
    assert_eq!(off["accounts_on"], false);
    let state = &off["providers"][0]["state"];
    assert_eq!(state.get("add_attempt"), None, "{state}");
    assert_eq!(state["busy"], false);
    assert_eq!(state["adding"], false);
    begun.send(()).unwrap();
    ends(&room, 1, "cancelled");
}

#[test]
fn the_list_never_shows_an_add_under_way_without_its_attempt() {
    let (ops, room, one, _) = lone_rig();
    let reply = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "one"}))
        .unwrap();
    assert_eq!(reply["state"], "waiting");
    let ender = ops.clone();
    *one.on_state.lock().unwrap() = Some(Box::new(move || {
        let asked = json!({"provider": "one", "attempt": 1});
        let ended = ender.call(ops::PROVIDERS_ADD_CANCEL, &asked).unwrap();
        assert_eq!(ended, json!({"cancelled": true}));
        true
    }));
    let state = listed(&ops);
    assert!(one.on_state.lock().unwrap().is_none(), "never read");
    assert_eq!(ends(&room, 1, "cancelled"), ["started", "cancelled"]);
    assert_eq!(state["adding"], false, "read while its add ended: {state}");
    assert_eq!(state.get("add_attempt"), None, "{state}");
    assert_eq!(state["busy"], false, "{state}");
}
