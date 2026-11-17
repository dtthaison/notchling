use crate::ask_testkit::{
    asker_at, base, open, open_with, person, rig, state_of, until, wait_as, ASKER, STRANGER, T0,
};
use crate::ask_waiters::{budget, Waiters};
use crate::event_assert::{of, text};
use crate::op_group::OpGroup;
use notchling_engine::ask::AskState;
use notchling_local_api::{ops, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const NOT_YOURS: &str = "Only the command that asked can wait for this answer.";

fn answer_one(r: &crate::ask_testkit::Rig, id: &str) {
    let done = person(r, ops::ASK_ANSWER, json!({"id": id, "choice": 2})).unwrap();
    assert_eq!(done["answered"], json!(true));
}

#[test]
fn wait_is_an_agent_op() {
    let r = rig();
    assert_eq!(r.ops.spec(ops::ASK_WAIT), Some(OpSpec::AGENT));
}

#[test]
fn wait_returns_at_once_when_answered() {
    let r = rig();
    let id = open(&r);
    answer_one(&r, &id);
    let started = Instant::now();
    let reply = wait_as(&r, ASKER, &id, 10_000).unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(reply["state"], json!("answered"));
    assert_eq!(reply["answer"], json!("b"));
    assert_eq!(reply["choice"], json!(2));
    assert_eq!(reply["by"], json!("mac"));
}

#[test]
fn wait_returns_open_after_wait_ms() {
    let r = rig();
    let id = open(&r);
    let started = Instant::now();
    let reply = wait_as(&r, ASKER, &id, 200).unwrap();
    let took = started.elapsed();
    assert_eq!(reply, json!({"state": "open"}));
    assert!(took >= Duration::from_millis(150), "{took:?}");
    assert!(took < Duration::from_secs(3), "{took:?}");
    assert_eq!(state_of(&r, &id), "open");
}

#[test]
fn wait_wakes_on_answer() {
    let r = rig();
    let id = open(&r);
    let ops = r.ops.clone();
    let answering = id.clone();
    let later = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        crate::ask_testkit::person_on(&ops, ops::ASK_ANSWER, json!({"id": answering, "choice": 1}))
    });
    let started = Instant::now();
    let reply = wait_as(&r, ASKER, &id, 20_000).unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(reply["state"], json!("answered"));
    assert_eq!(reply["choice"], json!(1));
    assert_eq!(later.join().unwrap().unwrap()["answered"], json!(true));
}

#[test]
fn delivered_on_reply_and_repeated_wait_gets_it_again() {
    let r = rig();
    let id = open(&r);
    answer_one(&r, &id);
    let first = wait_as(&r, ASKER, &id, 1_000).unwrap();
    assert_eq!(state_of(&r, &id), "delivered");
    let delivered = of(&r.sink, "session.ask")
        .into_iter()
        .filter(|e| text(e, "then").as_deref() == Some("delivered"))
        .collect::<Vec<_>>();
    assert_eq!(delivered.len(), 1);
    assert_eq!(text(&delivered[0], "waited").as_deref(), Some("0"));
    let again = wait_as(&r, ASKER, &id, 1_000).unwrap();
    assert_eq!(again, first);
    let all = of(&r.sink, "session.ask")
        .into_iter()
        .filter(|e| text(e, "then").as_deref() == Some("delivered"))
        .count();
    assert_eq!(all, 1);
    assert_eq!(
        wait_as(&r, STRANGER, &id, 100).unwrap_err().message,
        NOT_YOURS
    );
}

#[test]
fn delivery_publishes_and_counts_the_wait() {
    let r = rig();
    let id = open(&r);
    answer_one(&r, &id);
    r.clock
        .store(T0 + 7_000, std::sync::atomic::Ordering::SeqCst);
    wait_as(&r, ASKER, &id, 1_000).unwrap();
    let last = crate::ask_testkit::changes(&r).pop().unwrap();
    assert_eq!(last["state"], json!("delivered"));
    let delivered = of(&r.sink, "session.ask")
        .into_iter()
        .find(|e| text(e, "then").as_deref() == Some("delivered"))
        .unwrap();
    assert_eq!(text(&delivered, "waited").as_deref(), Some("7000"));
    assert_eq!(text(&delivered, "answer_chars").as_deref(), Some("1"));
}

#[test]
fn wait_from_another_process_is_refused() {
    let r = rig();
    let id = open(&r);
    let refused = wait_as(&r, STRANGER, &id, 100).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Forbidden);
    assert_eq!(refused.message, NOT_YOURS);
    let nobody = r
        .ops
        .call(
            &crate::acting_ops_tests::app(),
            ops::ASK_WAIT,
            &json!({"id": id, "waitMs": 100}),
        )
        .unwrap_err();
    assert_eq!(nobody.message, NOT_YOURS);
    assert_eq!(state_of(&r, &id), "open");
}

#[test]
fn same_pid_new_start_time_is_refused() {
    let r = rig();
    let id = open(&r);
    r.tree.restarted(ASKER, 9_999);
    let refused = wait_as(&r, ASKER, &id, 100).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Forbidden);
    assert_eq!(refused.message, NOT_YOURS);
}

#[test]
fn wait_on_an_unknown_question_is_a_bad_request() {
    let r = rig();
    let refused = wait_as(&r, ASKER, "nope", 100).unwrap_err();
    assert_eq!(refused.code, ErrorCode::BadRequest);
    assert_eq!(refused.message, "No such question.");
}

#[test]
fn dismissed_reaches_the_waiter() {
    let r = rig();
    let id = open(&r);
    let ops = r.ops.clone();
    let dismissing = id.clone();
    let later = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        crate::ask_testkit::person_on(&ops, ops::ASK_DISMISS, json!({"id": dismissing}))
    });
    let reply = wait_as(&r, ASKER, &id, 20_000).unwrap();
    assert_eq!(reply, json!({"state": "dismissed"}));
    later.join().unwrap().unwrap();
    assert_eq!(wait_as(&r, ASKER, &id, 100).unwrap(), reply);
}

#[test]
fn expired_reaches_the_waiter() {
    let r = rig();
    let mut args = base();
    args["wait"] = json!(true);
    args["deadlineMs"] = json!(T0 + 1_000);
    let id = open_with(&r, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let ops = r.ops.clone();
    let ticking = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        ops.tick(T0 + 2_000);
    });
    let reply = wait_as(&r, ASKER, &id, 20_000).unwrap();
    ticking.join().unwrap();
    assert_eq!(reply, json!({"state": "expired"}));
    assert_eq!(state_of(&r, &id), "expired");
}

#[test]
fn wait_is_served_while_acting_is_off() {
    let r = rig();
    let id = open(&r);
    answer_one(&r, &id);
    r.acting.set(false);
    let reply = wait_as(&r, ASKER, &id, 100).unwrap();
    assert_eq!(reply["state"], json!("answered"));
    let other = open_with(&r, base()).unwrap_err();
    assert_eq!(other.code, ErrorCode::Forbidden);
}

#[test]
fn waiting_flag_follows_the_waiter() {
    let r = rig();
    let id = open(&r);
    let listed = |r: &crate::ask_testkit::Rig| -> Value {
        person(r, ops::ASK_LIST, json!({})).unwrap()[0]["waiting"].clone()
    };
    assert_eq!(listed(&r), json!(false));
    let ops = r.ops.clone();
    let waiting = id.clone();
    let blocked = std::thread::spawn(move || {
        ops.call(
            &asker_at(ASKER),
            ops::ASK_WAIT,
            &json!({"id": waiting, "waitMs": 600}),
        )
    });
    until("a waiter connected", || listed(&r) == json!(true));
    blocked.join().unwrap().unwrap();
    assert_eq!(listed(&r), json!(false));
}

#[test]
fn close_wakes_a_waiter_and_later_waits_return_open() {
    let r = rig();
    let id = open(&r);
    let ops = r.ops.clone();
    let waiting = id.clone();
    let blocked = std::thread::spawn(move || {
        ops.call(
            &asker_at(ASKER),
            ops::ASK_WAIT,
            &json!({"id": waiting, "waitMs": 50_000}),
        )
    });
    until("a waiter connected", || {
        person(&r, ops::ASK_LIST, json!({})).unwrap()[0]["waiting"] == json!(true)
    });
    let started = Instant::now();
    r.ops.close();
    let reply = blocked.join().unwrap().unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(reply, json!({"state": "open"}));
    let started = Instant::now();
    assert_eq!(
        wait_as(&r, ASKER, &id, 50_000).unwrap(),
        json!({"state": "open"})
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn the_store_and_marks_are_free_while_a_waiter_blocks() {
    let r = rig();
    let id = open(&r);
    let ops = r.ops.clone();
    let waiting = id.clone();
    let blocked = std::thread::spawn(move || {
        ops.call(
            &asker_at(ASKER),
            ops::ASK_WAIT,
            &json!({"id": waiting, "waitMs": 5_000}),
        )
    });
    until("a waiter connected", || {
        person(&r, ops::ASK_LIST, json!({})).unwrap()[0]["waiting"] == json!(true)
    });
    let started = Instant::now();
    answer_one(&r, &id);
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(blocked.join().unwrap().unwrap()["state"], json!("answered"));
    assert_eq!(
        r.store.get(&id).unwrap().unwrap().state,
        AskState::Delivered
    );
}

#[test]
fn the_wait_budget_is_capped_at_fifty_seconds() {
    assert_eq!(budget(&json!({})), Duration::from_millis(50_000));
    assert_eq!(budget(&json!({"waitMs": 200})), Duration::from_millis(200));
    assert_eq!(
        budget(&json!({"waitMs": 900_000})),
        Duration::from_millis(50_000)
    );
    assert_eq!(budget(&json!({"waitMs": -5})), Duration::ZERO);
    assert_eq!(
        budget(&json!({"waitMs": "x"})),
        Duration::from_millis(50_000)
    );
}

#[test]
fn release_cannot_remove_the_slot_of_a_waiter_entering() {
    let waiters = Arc::new(Waiters::default());
    let (in_tx, in_rx) = channel();
    let (done_tx, done_rx) = channel::<()>();
    let done_rx = Mutex::new(done_rx);
    waiters.set_window(Arc::new(move || {
        in_tx.send(()).unwrap();
        let _ = done_rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_millis(300));
    }));
    let releaser = {
        let waiters = waiters.clone();
        std::thread::spawn(move || {
            in_rx.recv().unwrap();
            waiters.release("q");
            done_tx.send(()).unwrap();
        })
    };
    let here = waiters.enter("q");
    releaser.join().unwrap();
    assert!(waiters.connected("q"));
    waiters.wake("q");
    assert!(!here.block(here.mark(), Instant::now()));
}
