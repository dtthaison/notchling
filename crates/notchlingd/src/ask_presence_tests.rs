use crate::ask_testkit::{asker_at, item_of, open, rig, until, wait_as, ASKER};
use crate::op_group::OpGroup;
use notchling_engine::inbox::Waiter;
use notchling_local_api::ops;
use serde_json::json;
use std::sync::atomic::Ordering;

#[test]
fn a_connected_asker_makes_the_item_blocking_and_it_clears_after_the_grace() {
    let r = rig();
    let id = open(&r);
    assert_eq!(item_of(&r, &id).blocking, None);
    let started = r.clock.load(Ordering::SeqCst);
    let ops = r.ops.clone();
    let waiting = id.clone();
    let blocked = std::thread::spawn(move || {
        ops.call(
            &asker_at(ASKER),
            ops::ASK_WAIT,
            &json!({"id": waiting, "waitMs": 300}),
        )
    });
    until("blocking", || item_of(&r, &id).blocking == Some(true));
    let shown = item_of(&r, &id);
    assert_eq!(shown.waiter, Some(Waiter::Ask));
    assert_eq!(shown.waiting_since, Some(started));
    blocked.join().unwrap().unwrap();
    r.clock.fetch_add(4_000, Ordering::SeqCst);
    r.ops.tick(r.clock.load(Ordering::SeqCst));
    assert_eq!(item_of(&r, &id).blocking, Some(true));
    r.clock.fetch_add(2_000, Ordering::SeqCst);
    r.ops.tick(r.clock.load(Ordering::SeqCst));
    let cleared = item_of(&r, &id);
    assert_eq!(cleared.blocking, None);
    assert_eq!(cleared.waiter, None);
    assert_eq!(cleared.waiting_since, None);
}

#[test]
fn a_wait_of_zero_never_blocks() {
    let r = rig();
    let id = open(&r);
    wait_as(&r, ASKER, &id, 0).unwrap();
    r.ops.tick(r.clock.load(Ordering::SeqCst));
    assert_eq!(item_of(&r, &id).blocking, None);
}
