use crate::cluster_relay::Limits;
use crate::cluster_relay_testkit::*;
use notchling_engine::bridge::relay::{Cancelled, Relay, Routed};
use notchling_engine::cluster::persist;
use serde_json::json;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use std::time::Duration;

#[test]
fn an_old_entry_waits_for_the_phone_to_confirm_it() {
    let rig = rig(M1, M5, false);
    let old = held_id(message(&rig, "1", &format!("{M5}/job-7"), "old"));
    rig.clock.store(T0 + HOUR + 1, Ordering::SeqCst);
    go(&rig, M1, M5, true);
    rig.relay.flush();
    assert!(sent(&rig).is_empty(), "older than an hour waits");
    let list = rig.relay.list(DEV);
    assert!(list.entries[0].waiting);
    assert!(!rig.relay.confirm(DEV, "nope"));
    assert!(rig.relay.confirm(DEV, &old));
    rig.relay.flush();
    assert_eq!(sent(&rig).len(), 1, "confirmed, it goes now");
    assert!(rig.relay.list(DEV).entries.is_empty());
}

#[test]
fn confirm_answers_false_for_an_entry_that_is_not_waiting() {
    let rig = rig(M1, M5, false);
    let fresh = held_id(message(&rig, "1", &format!("{M5}/job-7"), "fresh"));
    assert!(
        !rig.relay.confirm(DEV, &fresh),
        "it isn't waiting for anyone"
    );
    rig.clock.store(T0 + HOUR + 1, Ordering::SeqCst);
    go(&rig, M1, M5, true);
    rig.relay.flush();
    assert!(
        sent(&rig).is_empty(),
        "the early confirm didn't release it once it grew old"
    );
    assert!(rig.relay.list(DEV).entries[0].waiting);
}

#[test]
fn a_held_entry_survives_a_handover_on_the_holding_machine() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "still here");
    let db = rig.dir.join(persist::CLUSTER_DB);
    assert!(db.exists());
    for name in ["cluster.db", "cluster.db-wal", "cluster.db-shm"] {
        let _ = std::fs::remove_file(rig.dir.join(name));
    }
    assert!(persist::load_held(&rig.dir).unwrap().list().is_empty());
    rig.relay.carry();
    let kept = persist::load_held(&rig.dir).unwrap().list();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].op, "session.message");
    let reborn = reborn(&rig);
    assert_eq!(reborn.list(DEV).entries.len(), 1, "a restart finds it too");
}

#[test]
fn held_entries_belong_to_the_phone_that_sent_them() {
    let rig = rig(M1, M5, false);
    let mine = held_id(message(&rig, "1", &format!("{M5}/job-7"), "private words"));
    assert!(rig.relay.list(OTHER).entries.is_empty());
    assert_eq!(rig.relay.cancel(OTHER, &mine), Cancelled::Unknown);
    assert_eq!(summaries(&rig), ["private words"], "not cancelled");
    rig.clock.store(T0 + HOUR + 1, Ordering::SeqCst);
    assert!(
        !rig.relay.confirm(OTHER, &mine),
        "nor confirmed by another phone"
    );
    go(&rig, M1, M5, true);
    rig.relay.flush();
    assert!(sent(&rig).is_empty(), "still waiting for its own phone");
    assert!(rig.relay.confirm(DEV, &mine));
    assert_eq!(rig.relay.cancel(OTHER, &mine), Cancelled::Unknown);
}

#[test]
fn the_same_request_from_two_phones_is_two_commands() {
    let rig = rig(M1, M5, false);
    let a = held_id(route_as(
        &rig,
        DEV,
        "1",
        None,
        &format!("{M5}/job-7"),
        "same",
    ));
    let b = held_id(route_as(
        &rig,
        OTHER,
        "1",
        None,
        &format!("{M5}/job-7"),
        "same",
    ));
    assert_ne!(a, b);
    assert_eq!(rig.relay.list(DEV).entries.len(), 1);
    assert_eq!(rig.relay.list(OTHER).entries.len(), 1);
}

#[test]
fn a_retry_after_the_answer_was_lost_is_not_sent_twice() {
    let rig = rig(M1, M5, false);
    let session = format!("{M5}/job-7");
    let held = held_id(keyed(&rig, "k-1", &session, "once"));
    go(&rig, M1, M5, true);
    rig.relay.flush();
    assert_eq!(sent(&rig).len(), 1);
    let retry = keyed(&rig, "k-1", &session, "once");
    assert_eq!(
        retry,
        Routed::Done(json!({"sent": true, "relayed": true})),
        "answered from the delivered record"
    );
    assert_eq!(sent(&rig).len(), 1, "the primary isn't asked again");
    assert_eq!(rig.relay.cancel(DEV, &held), Cancelled::Delivered);
}

#[test]
fn a_retry_of_a_command_sent_straight_is_not_sent_twice() {
    let rig = rig(M1, M5, true);
    let session = format!("{M5}/job-7");
    assert!(matches!(
        keyed(&rig, "k-1", &session, "once"),
        Routed::Done(_)
    ));
    assert!(matches!(
        keyed(&rig, "k-1", &session, "once"),
        Routed::Done(_)
    ));
    assert_eq!(sent(&rig).len(), 1);
    assert!(matches!(
        route_as(&rig, DEV, "5", None, &session, "twice"),
        Routed::Done(_)
    ));
    assert!(matches!(
        route_as(&rig, DEV, "5", None, &session, "twice"),
        Routed::Done(_)
    ));
    assert_eq!(sent(&rig).len(), 2, "the same id and words, once");
}

#[test]
fn a_retry_of_a_waiting_entry_is_not_sent_around_it() {
    let rig = rig(M1, M5, false);
    let session = format!("{M5}/job-7");
    let old = held_id(keyed(&rig, "k-1", &session, "old"));
    rig.clock.store(T0 + HOUR + 1, Ordering::SeqCst);
    go(&rig, M1, M5, true);
    rig.relay.flush();
    assert!(sent(&rig).is_empty());
    let retry = held_id(keyed(&rig, "k-1", &session, "old"));
    assert_eq!(retry, old, "the same entry, still waiting");
    assert!(sent(&rig).is_empty());
    assert!(rig.relay.confirm(DEV, &old));
    rig.relay.flush();
    assert_eq!(sent(&rig).len(), 1, "it goes once");
}

#[test]
fn a_different_command_with_a_repeated_id_is_held_not_swallowed() {
    let rig = rig(M1, M5, false);
    let session = format!("{M5}/job-7");
    let first = held_id(message(&rig, "1", &session, "first"));
    let second = held_id(message(&rig, "1", &session, "second"));
    assert_ne!(first, second);
    assert_eq!(summaries(&rig), ["first", "second"]);
    let keyed_one = held_id(keyed(&rig, "k-1", &session, "third"));
    let keyed_two = held_id(keyed(&rig, "k-2", &session, "third"));
    assert_ne!(
        keyed_one, keyed_two,
        "two keys, two commands, even with the same words"
    );
    let a = held_id(message(&rig, "", &session, "yes"));
    let b = held_id(message(&rig, "", &session, "yes"));
    assert_ne!(a, b, "no id and no key: nothing to tell them apart");
    assert_eq!(rig.relay.list(DEV).entries.len(), 6);
}

#[test]
fn a_restarted_phone_reusing_an_old_id_for_a_new_command_is_not_swallowed() {
    let rig = rig(M1, M5, true);
    let session = format!("{M5}/job-7");
    assert!(matches!(
        message(&rig, "1", &session, "before the restart"),
        Routed::Done(_)
    ));
    assert!(matches!(
        message(&rig, "1", &session, "after the restart"),
        Routed::Done(_)
    ));
    assert_eq!(
        texts(&rig),
        [json!("before the restart"), json!("after the restart")]
    );
}

#[test]
fn the_queue_is_bounded_per_phone_and_in_total() {
    let rig = rig_with(
        M1,
        M5,
        false,
        Limits {
            total: 3,
            device: 2,
        },
    );
    let session = format!("{M5}/job-7");
    held_id(keyed(&rig, "a", &session, "a"));
    let b = held_id(keyed(&rig, "b", &session, "b"));
    let Routed::Fail(failure) = keyed(&rig, "c", &session, "c") else {
        panic!("refused past the phone's share");
    };
    assert!(failure.message.contains("m5"), "{}", failure.message);
    assert_eq!(rig.relay.list(DEV).entries.len(), 2);
    assert_eq!(
        held_id(keyed(&rig, "b", &session, "b")),
        b,
        "a retry still answers"
    );
    held_id(route_as(&rig, OTHER, "1", Some("o1"), &session, "o1"));
    assert!(
        matches!(
            route_as(&rig, OTHER, "2", Some("o2"), &session, "o2"),
            Routed::Fail(_)
        ),
        "the total is three"
    );
}

#[test]
fn a_primary_that_hangs_does_not_stall_the_hub_tick_or_the_router() {
    let rig = rig(M1, M5, false);
    let session = format!("{M5}/job-7");
    message(&rig, "1", &session, "one");
    go(&rig, M1, M5, true);
    let (entered_tx, entered) = channel();
    let (release, release_rx) = channel();
    *rig.net.hang.lock().unwrap() = Some((entered_tx, release_rx));
    rig.relay.start();
    rig.relay.tick();
    entered
        .recv_timeout(Duration::from_secs(10))
        .expect("the worker is inside the call");
    rig.relay.tick();
    let behind = message(&rig, "2", &session, "two");
    assert!(
        matches!(behind, Routed::Held(_)),
        "held behind the one in flight, not sent around it and not blocked"
    );
    let shown = rig.relay.list(DEV);
    assert_eq!(shown.entries.len(), 2, "the one in flight is still listed");
    release.send(()).unwrap();
    rig.relay.settle();
    rig.relay.flush();
    rig.relay.settle();
    assert_eq!(texts(&rig), [json!("one"), json!("two")]);
    assert!(rig.relay.list(DEV).entries.is_empty());
}

#[test]
fn a_tick_with_nothing_held_does_not_ask_the_node() {
    let rig = rig(M1, M5, true);
    rig.relay.start();
    rig.relay.tick();
    rig.relay.settle();
    assert!(sent(&rig).is_empty());
}

#[test]
fn a_held_entry_is_listed_with_its_key_and_cancelled_by_it() {
    let rig = rig(M1, M5, false);
    let session = format!("{M5}/job-7");
    keyed(&rig, "k-1", &session, "first");
    message(&rig, "5", &session, "no key");
    let list = rig.relay.list(DEV);
    assert_eq!(list.entries[0].key.as_deref(), Some("k-1"));
    assert_eq!(list.entries[1].key, None);
    assert_eq!(rig.relay.cancel_key(OTHER, "k-1"), Cancelled::Unknown);
    assert_eq!(rig.relay.list(DEV).entries.len(), 2, "another phone's key");
    assert_eq!(rig.relay.cancel_key(DEV, "k-9"), Cancelled::Unknown);
    assert_eq!(rig.relay.cancel_key(DEV, "k-1"), Cancelled::Dropped);
    assert_eq!(summaries(&rig), ["no key"]);
    go(&rig, M1, M5, true);
    rig.relay.flush();
    assert_eq!(
        texts(&rig),
        [json!("no key")],
        "the cancelled one never went"
    );
}

#[test]
fn cancel_by_key_after_it_was_sent_says_delivered() {
    let rig = rig(M1, M5, true);
    let session = format!("{M5}/job-7");
    assert!(matches!(
        keyed(&rig, "k-1", &session, "straight"),
        Routed::Done(_)
    ));
    assert_eq!(rig.relay.cancel_key(DEV, "k-1"), Cancelled::Delivered);
    assert_eq!(rig.relay.cancel_key(OTHER, "k-1"), Cancelled::Unknown);
    go(&rig, M1, M5, false);
    let held = held_id(keyed(&rig, "k-2", &session, "later"));
    go(&rig, M1, M5, true);
    rig.relay.flush();
    assert_eq!(rig.relay.cancel_key(DEV, "k-2"), Cancelled::Delivered);
    assert_eq!(rig.relay.cancel(DEV, &held), Cancelled::Delivered);
}

#[test]
fn an_entry_the_worker_is_sending_cannot_be_cancelled() {
    let rig = rig(M1, M5, false);
    let session = format!("{M5}/job-7");
    let held = held_id(keyed(&rig, "k-1", &session, "one"));
    go(&rig, M1, M5, true);
    let (entered_tx, entered) = channel();
    let (release, release_rx) = channel();
    *rig.net.hang.lock().unwrap() = Some((entered_tx, release_rx));
    rig.relay.start();
    rig.relay.tick();
    entered
        .recv_timeout(Duration::from_secs(10))
        .expect("the worker is inside the call");
    assert_eq!(rig.relay.cancel_key(DEV, "k-1"), Cancelled::InFlight);
    assert_eq!(rig.relay.cancel(DEV, &held), Cancelled::InFlight);
    assert_eq!(rig.relay.cancel_key(OTHER, "k-1"), Cancelled::Unknown);
    assert_eq!(rig.relay.cancel(OTHER, &held), Cancelled::Unknown);
    release.send(()).unwrap();
    rig.relay.settle();
    assert_eq!(texts(&rig), [json!("one")], "it went exactly once");
    assert_eq!(rig.relay.cancel_key(DEV, "k-1"), Cancelled::Delivered);
}
