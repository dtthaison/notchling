use super::*;
use crate::acting::Acting;
use crate::acting_ops_tests::{app, row, sessions, FakeRoom};
use crate::consent_testkit::{shell, verified};
use notchling_engine::outbox::OutboxStore;
use notchling_engine::sessions::Roots;
use notchling_local_api::bind_identity;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

const OWN: &str = "5b6c7d8e-1f2a-4b3c-8d4e-9f0a1b2c3d4e";
const OTHER_TEXT: &str = "planted message of another session";
const OWN_TEXT: &str = "my own message";

fn rig() -> OutboxOps {
    let mut all = sessions();
    all.as_array_mut().unwrap().push(row(
        OWN,
        json!({"job": "uuid0001"}),
        "completed",
        Some(&format!("/tmp/none/{OWN}.jsonl")),
    ));
    let room = Arc::new(FakeRoom::default());
    room.app.store(true, Ordering::SeqCst);
    let roots = Roots {
        main_dir: PathBuf::from("/nonexistent/claude"),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    OutboxOps::new(
        Arc::new(OutboxStore::in_memory_for_tests()),
        Arc::new(Mutex::new(all)),
        roots,
        Arc::new(Acting::in_memory(true)),
        room,
        None,
    )
}

fn plant(ops: &OutboxOps, id: &str, text: &str) {
    ops.call(&app(), ops::SESSION_SEND, &json!({"id": id, "text": text}))
        .unwrap();
}

fn mine(ops: &OutboxOps, args: Value) -> Value {
    ops.call(&app(), ops::OUTBOX_MINE, &args).unwrap()
}

fn planted() -> OutboxOps {
    let ops = rig();
    plant(&ops, OWN, OWN_TEXT);
    plant(&ops, "job-idle", OTHER_TEXT);
    plant(&ops, "job-a", "a second planted message");
    ops
}

#[test]
fn a_verified_session_reads_its_own_messages_and_counts_the_rest() {
    let ops = planted();
    let _who = bind_identity(Some(verified(OWN)));
    let reply = mine(&ops, json!({}));
    let rows = reply["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["text"], OWN_TEXT);
    assert_eq!(reply["others"], json!({"messages": 2, "sessions": 2}));
    assert!(reply.get("reason").is_none());
    let text = reply.to_string();
    for planted in [
        OTHER_TEXT,
        "a second planted message",
        "idle0001",
        "abcd1234",
    ] {
        assert!(!text.contains(planted), "{planted} leaked: {text}");
    }
}

#[test]
fn receipts_carry_state_and_never_text_and_only_for_the_own_session() {
    let ops = planted();
    let _who = bind_identity(Some(verified(OWN)));
    let reply = mine(&ops, json!({"receipts": true}));
    let rows = reply["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].get("text").is_none());
    assert_eq!(rows[0]["state"], "queued");
    let text = reply.to_string();
    assert!(
        !text.contains(OWN_TEXT) && !text.contains(OTHER_TEXT),
        "{text}"
    );
}

#[test]
fn another_verified_session_sees_none_of_it() {
    let ops = planted();
    let _who = bind_identity(Some(verified("a-session-with-no-messages")));
    let reply = mine(&ops, json!({}));
    assert_eq!(reply["rows"], json!([]));
    let text = reply.to_string();
    assert!(
        !text.contains(OWN_TEXT) && !text.contains(OTHER_TEXT),
        "{text}"
    );
}

#[test]
fn a_caller_with_no_verified_session_gets_nothing_and_the_reason() {
    let ops = planted();
    let mut unverified = verified(OWN);
    unverified.verified = false;
    for who in [None, Some(shell("Terminal")), Some(unverified)] {
        let _bound = bind_identity(who);
        for args in [json!({}), json!({"receipts": true})] {
            let reply = mine(&ops, args);
            assert_eq!(reply["rows"], json!([]));
            assert_eq!(reply["reason"], "no_session");
            assert_eq!(reply["others"], json!({"messages": 0, "sessions": 0}));
            let text = reply.to_string();
            assert!(
                !text.contains(OWN_TEXT) && !text.contains(OTHER_TEXT),
                "{text}"
            );
        }
    }
}
