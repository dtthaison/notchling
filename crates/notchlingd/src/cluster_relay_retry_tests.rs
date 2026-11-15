use crate::cluster_relay::{NOTICES_PER_DEVICE, STAY_CAP_MS, STAY_FIRST_MS};
use crate::cluster_relay_testkit::*;
use notchling_engine::bridge::relay::{Relay, Routed};
use notchling_engine::cluster::held::HeldQueue;
use notchling_engine::cluster::persist;
use notchling_engine::cluster::wire::WireError;
use serde_json::json;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Barrier};

fn blank_disk(rig: &Rig) {
    persist::save_held(&rig.dir, &HeldQueue::default()).unwrap();
}

fn on_disk(rig: &Rig) -> usize {
    persist::load_held(&rig.dir).unwrap().list().len()
}

fn tick_clock(rig: &Rig, ms: i64) {
    rig.clock.fetch_add(ms, Ordering::SeqCst);
}

#[test]
fn a_command_for_a_session_not_on_the_primary_backs_off_instead_of_churning() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "keep going");
    go(&rig, M1, M5, true);
    rig.rows.lock().unwrap().clear();
    rig.relay.flush();
    assert_eq!(on_disk(&rig), 1);
    blank_disk(&rig);
    rig.relay.flush();
    rig.relay.flush();
    assert_eq!(on_disk(&rig), 0, "inside the backoff nothing is rewritten");
    assert!(sent(&rig).is_empty());
    tick_clock(&rig, STAY_FIRST_MS + 1);
    rig.relay.flush();
    assert_eq!(on_disk(&rig), 1, "tried again once the wait is over");
    blank_disk(&rig);
    tick_clock(&rig, STAY_FIRST_MS + 1);
    rig.relay.flush();
    assert_eq!(on_disk(&rig), 0, "the second wait is longer");
    tick_clock(&rig, STAY_FIRST_MS);
    rig.relay.flush();
    assert_eq!(on_disk(&rig), 1);
    assert_eq!(rig.relay.list(DEV).entries.len(), 1, "never dropped");
    assert!(rig.relay.list(DEV).notices.is_empty());
}

#[test]
fn the_backoff_stops_growing_at_its_cap() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "keep going");
    go(&rig, M1, M5, true);
    rig.rows.lock().unwrap().clear();
    for _ in 0..8 {
        tick_clock(&rig, STAY_CAP_MS + 1);
        rig.relay.flush();
    }
    blank_disk(&rig);
    tick_clock(&rig, STAY_CAP_MS + 1);
    rig.relay.flush();
    assert_eq!(on_disk(&rig), 1, "one cap is always enough");
}

#[test]
fn a_confirmed_old_entry_for_a_gone_session_backs_off_too() {
    let rig = rig(M1, M5, false);
    let old = held_id(message(&rig, "1", &format!("{M5}/job-7"), "old"));
    tick_clock(&rig, HOUR + 1);
    go(&rig, M1, M5, true);
    rig.rows.lock().unwrap().clear();
    assert!(rig.relay.confirm(DEV, &old));
    rig.relay.flush();
    blank_disk(&rig);
    rig.relay.flush();
    rig.relay.flush();
    assert_eq!(on_disk(&rig), 0);
}

#[test]
fn a_session_that_comes_back_is_sent_to_at_once() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "keep going");
    go(&rig, M1, M5, true);
    rig.rows.lock().unwrap().clear();
    rig.relay.flush();
    *rig.rows.lock().unwrap() = vec![format!("{M5}/job-7")];
    rig.relay.flush();
    assert_eq!(sent(&rig).len(), 1, "no waiting out the backoff");
    assert!(rig.relay.list(DEV).entries.is_empty());
}

#[test]
fn a_cancelled_entry_leaves_no_backoff_behind() {
    let rig = rig(M1, M5, false);
    let id = held_id(message(&rig, "1", &format!("{M5}/job-7"), "keep going"));
    go(&rig, M1, M5, true);
    rig.rows.lock().unwrap().clear();
    rig.relay.flush();
    rig.relay.cancel(DEV, &id);
    rig.relay.flush();
    assert!(rig.relay.list(DEV).entries.is_empty());
}

#[test]
fn a_dropped_command_is_not_told_it_was_relayed_on_a_retry() {
    let rig = rig(M1, M5, false);
    let session = format!("{M5}/job-7");
    assert!(matches!(
        message(&rig, "1", &session, "one"),
        Routed::Held(_)
    ));
    go(&rig, M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(WireError::new("failed", "refused"));
    rig.relay.flush();
    assert_eq!(rig.relay.list(DEV).notices.len(), 1);
    let again = message(&rig, "1", &session, "one");
    assert!(
        !matches!(again, Routed::Done(_)),
        "it was dropped, not sent: {again:?}"
    );
}

#[test]
fn a_command_whose_answer_timed_out_is_still_told_relayed_on_a_retry() {
    let rig = rig(M1, M5, false);
    let session = format!("{M5}/job-7");
    message(&rig, "1", &session, "one");
    go(&rig, M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(WireError::new("timeout", "slow"));
    rig.relay.flush();
    assert_eq!(rig.relay.list(DEV).notices.len(), 1);
    assert!(matches!(
        message(&rig, "1", &session, "one"),
        Routed::Done(_)
    ));
    assert_eq!(sent(&rig).len(), 1, "never sent twice");
}

#[test]
fn one_phones_notices_never_push_out_another_phones() {
    let rig = rig(M1, M5, false);
    let session = format!("{M5}/job-7");
    route_as(&rig, OTHER, "1", None, &session, "theirs");
    for n in 0..(NOTICES_PER_DEVICE + 15) {
        message(&rig, &format!("m{n}"), &session, &format!("mine {n}"));
    }
    go(&rig, M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(WireError::new("failed", "refused"));
    rig.relay.flush();
    rig.relay.flush();
    assert_eq!(rig.relay.list(OTHER).notices.len(), 1);
    assert_eq!(rig.relay.list(DEV).notices.len(), NOTICES_PER_DEVICE);
}

#[test]
fn one_phone_cannot_use_up_the_delivered_record_of_another() {
    let rig = rig(M1, M5, true);
    let session = format!("{M5}/job-7");
    assert!(matches!(
        route_as(&rig, OTHER, "1", Some("theirs"), &session, "theirs"),
        Routed::Done(_)
    ));
    for n in 0..300 {
        message(&rig, &format!("m{n}"), &session, &format!("mine {n}"));
    }
    let before = sent(&rig).len();
    assert!(matches!(
        route_as(&rig, OTHER, "1", Some("theirs"), &session, "theirs"),
        Routed::Done(_)
    ));
    assert_eq!(sent(&rig).len(), before, "their retry is still recognised");
}

#[test]
fn two_threads_routing_one_key_send_it_once() {
    let rig = Arc::new(rig(M1, M5, true));
    let session = format!("{M5}/job-7");
    for round in 0..150 {
        let barrier = Arc::new(Barrier::new(4));
        let key = format!("k-{round}");
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let (rig, barrier, session, key) =
                    (rig.clone(), barrier.clone(), session.clone(), key.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    keyed(&rig, &key, &session, "same words")
                })
            })
            .collect();
        for thread in threads {
            assert!(matches!(thread.join().unwrap(), Routed::Done(_)));
        }
        assert_eq!(sent(&rig).len(), round + 1, "round {round} sent twice");
    }
    assert_eq!(texts(&rig).len(), 150);
    assert_eq!(json!(texts(&rig)[0]), json!("same words"));
}

#[test]
fn a_command_relayed_straight_is_not_sent_again_after_a_restart() {
    let rig = rig(M1, M5, true);
    let session = format!("{M5}/job-7");
    assert!(matches!(
        keyed(&rig, "k-1", &session, "straight"),
        Routed::Done(_)
    ));
    let reborn = reborn(&rig);
    let act = notchling_engine::bridge::relay::HeldAct::Message {
        session_id: session.clone(),
        text: "straight".to_string(),
    };
    let found = crate::cluster_relay_testkit::session(&session);
    let again = reborn.route(&notchling_engine::bridge::relay::Call {
        device: DEV,
        request: "7",
        key: Some("k-1"),
        act: &act,
        session: Some(&found),
    });
    assert!(matches!(again, Routed::Done(_)), "{again:?}");
    assert_eq!(sent(&rig).len(), 1, "not sent twice");
}

#[test]
fn a_straight_send_that_is_refused_leaves_no_record_after_a_restart() {
    let rig = rig(M1, M5, true);
    let session = format!("{M5}/job-7");
    *rig.net.reply.lock().unwrap() = Err(WireError::new("failed", "refused"));
    assert!(matches!(
        keyed(&rig, "k-1", &session, "straight"),
        Routed::Fail(_)
    ));
    let reborn = reborn(&rig);
    *rig.net.reply.lock().unwrap() = Ok(json!({}));
    let act = notchling_engine::bridge::relay::HeldAct::Message {
        session_id: session.clone(),
        text: "straight".to_string(),
    };
    let found = crate::cluster_relay_testkit::session(&session);
    reborn.route(&notchling_engine::bridge::relay::Call {
        device: DEV,
        request: "7",
        key: Some("k-1"),
        act: &act,
        session: Some(&found),
    });
    assert_eq!(sent(&rig).len(), 2, "the retry really went");
}
