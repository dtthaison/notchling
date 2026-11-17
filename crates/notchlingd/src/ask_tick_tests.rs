use crate::ask_testkit::{
    asker_at, base, changes, new_row, open, open_with, person, rig, state_of, until, ASKER, T0,
};
use crate::event_assert::{of, text};
use crate::op_group::OpGroup;
use notchling_engine::ask::{AskState, KEEP_CLOSED_MS};
use notchling_local_api::{ops, topics};
use serde_json::json;
use std::sync::atomic::Ordering;
use std::time::Duration;

fn thens(r: &crate::ask_testkit::Rig) -> Vec<String> {
    of(&r.sink, "session.ask")
        .iter()
        .filter_map(|e| text(e, "then"))
        .collect()
}

fn blocking(r: &crate::ask_testkit::Rig, deadline: i64) -> String {
    let mut args = base();
    args["wait"] = json!(true);
    args["deadlineMs"] = json!(deadline);
    open_with(r, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn dead_asker_is_gone() {
    let r = rig();
    let id = open(&r);
    r.ops.tick(T0 + 1_000);
    assert_eq!(state_of(&r, &id), "open");
    r.tree.end(ASKER);
    let before = changes(&r).len();
    r.clock.store(T0 + 2_000, Ordering::SeqCst);
    r.ops.tick(T0 + 2_000);
    let row = r.store.get(&id).unwrap().unwrap();
    assert_eq!(row.state, AskState::Gone);
    assert_eq!(row.closed_ms, Some(T0 + 2_000));
    let after = changes(&r);
    assert_eq!(after.len(), before + 1);
    assert_eq!(after.last().unwrap()["state"], json!("gone"));
    assert_eq!(thens(&r), vec!["asked", "gone"]);
    r.ops.tick(T0 + 3_000);
    assert_eq!(thens(&r), vec!["asked", "gone"]);
}

#[test]
fn a_connected_waiter_is_not_checked() {
    let r = rig();
    let id = open(&r);
    let ops = r.ops.clone();
    let waiting = id.clone();
    let blocked = std::thread::spawn(move || {
        ops.call(
            &asker_at(ASKER),
            ops::ASK_WAIT,
            &json!({"id": waiting, "waitMs": 800}),
        )
    });
    until("a waiter connected", || {
        person(&r, ops::ASK_LIST, json!({})).unwrap()[0]["waiting"] == json!(true)
    });
    r.tree.end(ASKER);
    r.ops.tick(T0 + 5_000);
    assert_eq!(state_of(&r, &id), "open");
    blocked.join().unwrap().unwrap();
    r.ops.tick(T0 + 6_000);
    assert_eq!(state_of(&r, &id), "gone");
}

#[test]
fn reused_pid_is_gone() {
    let r = rig();
    let id = open(&r);
    r.tree.restarted(ASKER, 777);
    r.ops.tick(T0 + 1_000);
    assert_eq!(state_of(&r, &id), "gone");
}

#[test]
fn an_answered_row_of_a_dead_asker_is_gone() {
    let r = rig();
    let id = open(&r);
    person(&r, ops::ASK_ANSWER, json!({"id": id, "choice": 1})).unwrap();
    r.tree.end(ASKER);
    r.ops.tick(T0 + 1_000);
    assert_eq!(state_of(&r, &id), "gone");
}

#[test]
fn a_row_without_a_start_time_is_judged_by_its_pid() {
    let r = rig();
    let alive = r.store.insert(&new_row("s", "a", ASKER), T0).unwrap();
    let dead = r.store.insert(&new_row("s", "b", 31_337), T0).unwrap();
    r.ops.tick(T0 + 1_000);
    assert_eq!(state_of(&r, &alive.id), "open");
    assert_eq!(state_of(&r, &dead.id), "gone");
}

#[test]
fn a_blocking_row_past_its_deadline_expires() {
    let r = rig();
    let id = blocking(&r, T0 + 10_000);
    r.ops.tick(T0 + 9_999);
    assert_eq!(state_of(&r, &id), "open");
    r.clock.store(T0 + 10_000, Ordering::SeqCst);
    r.ops.tick(T0 + 10_000);
    assert_eq!(state_of(&r, &id), "expired");
    assert_eq!(changes(&r).last().unwrap()["state"], json!("expired"));
    assert_eq!(thens(&r), vec!["asked", "expired"]);
}

#[test]
fn a_background_row_never_expires() {
    let r = rig();
    let mut args = base();
    args["deadlineMs"] = json!(T0 + 1);
    let id = open_with(&r, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    r.ops.tick(T0 + 100_000_000);
    assert_eq!(state_of(&r, &id), "open");
}

#[test]
fn an_answered_blocking_row_is_handed_over_past_its_deadline() {
    let r = rig();
    let id = blocking(&r, T0 + 10_000);
    person(&r, ops::ASK_ANSWER, json!({"id": id, "choice": 1})).unwrap();
    r.ops.tick(T0 + 20_000);
    assert_eq!(state_of(&r, &id), "answered");
}

#[test]
fn sweep_runs_hourly_and_publishes_removed() {
    let r = rig();
    let id = open(&r);
    let other = r.store.insert(&new_row("s2", "z", ASKER), T0).unwrap();
    r.store
        .close(
            &id,
            &[AskState::Open],
            AskState::Dismissed,
            T0 - KEEP_CLOSED_MS - 1,
        )
        .unwrap();
    r.ops.tick(T0);
    let removed = |r: &crate::ask_testkit::Rig| {
        r.room
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == topics::ASK_REMOVED)
            .map(|(_, data)| data.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        removed(&r),
        vec![json!({"id": id, "session": crate::acting_ops_tests::SESSION})]
    );
    assert!(r.store.get(&id).unwrap().is_none());
    r.store
        .close(
            &other.id,
            &[AskState::Open],
            AskState::Dismissed,
            T0 - KEEP_CLOSED_MS - 1,
        )
        .unwrap();
    r.ops.tick(T0 + 60_000);
    assert_eq!(removed(&r).len(), 1);
    assert!(r.store.get(&other.id).unwrap().is_some());
    r.ops.tick(T0 + 3_600_000);
    assert_eq!(removed(&r).len(), 2);
    assert_eq!(removed(&r)[1], json!({"id": other.id, "session": "s2"}));
    assert!(r.store.get(&other.id).unwrap().is_none());
}

#[test]
fn the_loop_ticks_and_close_stops_it() {
    let r = rig();
    let id = open(&r);
    r.ops.close();
    r.ops.spawn_loop_every(Duration::from_millis(20));
    r.ops.spawn_loop_every(Duration::from_millis(20));
    r.tree.end(ASKER);
    until("the loop closed the row", || state_of(&r, &id) == "gone");
    r.ops.close();
    r.ops.close();
}

#[test]
fn the_loop_ends_when_the_ops_are_dropped() {
    let r = rig();
    r.ops.spawn_loop_every(Duration::from_millis(10));
    let weak = std::sync::Arc::downgrade(&r.ops);
    drop(r);
    until("the ops were freed", || weak.upgrade().is_none());
}
