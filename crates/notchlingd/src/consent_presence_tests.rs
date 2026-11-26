use super::tests::{at, build, rig, Rig};
use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_testkit::item_for;
use crate::limits_ops_tests::TempDir;
use notchling_engine::consent::SETTING;
use notchling_events::MemorySink;
use std::sync::atomic::Ordering;

fn poll(r: &Rig, id: &str, ms: u64) -> std::thread::JoinHandle<Value> {
    let consent = r.consent.clone();
    let args = json!({"id": id, "waitMs": ms});
    std::thread::spawn(move || {
        consent
            .call(&at(1), ops::CONSENT_GET, &args)
            .unwrap_or(Value::Null)
    })
}

fn snapshot(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(next) = todo.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                todo.push(path);
            } else {
                let bytes = std::fs::read(&path).unwrap_or_default();
                files.push((path.display().to_string(), bytes));
            }
        }
    }
    files.sort();
    files
}

fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let stop = std::time::Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(std::time::Instant::now() < stop, "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_poll_with_wait_makes_the_item_blocking_and_it_clears_after_the_grace() {
    let r = rig("presence-poll");
    let row = r.open(1, SETTING, "limitMode");
    let before = item_for(&r.inbox, &row.id);
    assert!(before["blocking"].is_null());
    let started = r.clock.load(Ordering::SeqCst);
    let poll = poll(&r, &row.id, 300);
    until("blocking", || {
        item_for(&r.inbox, &row.id)["blocking"] == true
    });
    let shown = item_for(&r.inbox, &row.id);
    assert_eq!(shown["waiter"], "cli");
    assert_eq!(shown["waiting_since"], started);
    poll.join().unwrap();
    r.clock.fetch_add(4_000, Ordering::SeqCst);
    r.consent.tick();
    assert_eq!(item_for(&r.inbox, &row.id)["blocking"], true);
    r.clock.fetch_add(2_000, Ordering::SeqCst);
    r.consent.tick();
    let cleared = item_for(&r.inbox, &row.id);
    assert!(cleared["blocking"].is_null());
    assert!(cleared["waiter"].is_null());
    assert!(cleared["waiting_since"].is_null());
    assert_eq!(cleared["state"], "open");
}

#[test]
fn back_to_back_polls_keep_one_waiting_since() {
    let r = rig("presence-since");
    let row = r.open(1, SETTING, "limitMode");
    let started = r.clock.load(Ordering::SeqCst);
    poll(&r, &row.id, 100).join().unwrap();
    r.clock.fetch_add(25_000, Ordering::SeqCst);
    r.consent.tick();
    let second = poll(&r, &row.id, 100);
    until("blocking", || {
        item_for(&r.inbox, &row.id)["blocking"] == true
    });
    second.join().unwrap();
    r.clock.fetch_add(1_000, Ordering::SeqCst);
    r.consent.tick();
    let shown = item_for(&r.inbox, &row.id);
    assert_eq!(shown["blocking"], true);
    assert_eq!(shown["waiting_since"], started + 25_000);
}

#[test]
fn a_get_without_wait_is_never_blocking() {
    let r = rig("presence-nowait");
    let row = r.open(1, SETTING, "limitMode");
    let got = r
        .consent
        .call(&at(1), ops::CONSENT_GET, &json!({"id": row.id}))
        .unwrap();
    assert_eq!(got["state"], "waiting");
    r.consent.tick();
    assert!(item_for(&r.inbox, &row.id)["blocking"].is_null());
    r.consent
        .call(
            &at(1),
            ops::CONSENT_GET,
            &json!({"id": row.id, "waitMs": 0}),
        )
        .unwrap();
    r.consent.tick();
    assert!(item_for(&r.inbox, &row.id)["blocking"].is_null());
}

#[test]
fn an_answered_request_closes_its_item_without_a_live_mark() {
    let r = rig("presence-answered");
    let row = r.open(1, SETTING, "limitMode");
    let poll = poll(&r, &row.id, 5_000);
    until("blocking", || {
        item_for(&r.inbox, &row.id)["blocking"] == true
    });
    let item_id = item_for(&r.inbox, &row.id)["id"]
        .as_str()
        .unwrap()
        .to_string();
    r.inbox
        .decide(&item_id, notchling_engine::inbox::ActionId::Deny, "mac")
        .unwrap();
    poll.join().unwrap();
    let closed = item_for(&r.inbox, &row.id);
    assert_ne!(closed["state"], "open");
    assert!(closed["blocking"].is_null());
}

#[test]
fn a_restart_brings_the_item_back_not_blocking_and_a_new_poll_blocks_since_creation() {
    let r = rig("presence-restart");
    let row = r.open(1, SETTING, "limitMode");
    poll(&r, &row.id, 100).join().unwrap();
    r.consent.close();
    r.clock.fetch_add(30_000, Ordering::SeqCst);
    let tmp = TempDir::new("consent-presence-restart-2");
    let room = Arc::new(FakeRoom::default());
    let sink = Arc::new(MemorySink::new());
    let (inbox, again) = build(&tmp, &room, &r.store, &sink, &r.clock);
    again.load();
    let item = item_for(&inbox, &row.id);
    assert_eq!(item["state"], "open");
    assert!(item["blocking"].is_null());
    assert!(item["waiting_since"].is_null());
    let args = json!({"id": row.id, "waitMs": 300});
    let consent = again.clone();
    let poll = std::thread::spawn(move || consent.call(&at(1), ops::CONSENT_GET, &args));
    until("blocking", || item_for(&inbox, &row.id)["blocking"] == true);
    let shown = item_for(&inbox, &row.id);
    assert_eq!(shown["waiting_since"], row.created_ms);
    assert_eq!(shown["waiter"], "cli");
    let _ = poll.join();
    again.close();
}

#[test]
fn a_presence_change_writes_nothing_to_disk() {
    let r = rig("presence-nowrite");
    let row = r.open(1, SETTING, "limitMode");
    r.inbox.flush();
    let before = snapshot(&r._tmp.path("inbox"));
    let poll = poll(&r, &row.id, 200);
    until("blocking", || {
        item_for(&r.inbox, &row.id)["blocking"] == true
    });
    poll.join().unwrap();
    r.clock.fetch_add(6_000, Ordering::SeqCst);
    r.consent.tick();
    r.inbox.flush();
    assert_eq!(snapshot(&r._tmp.path("inbox")), before);
}
