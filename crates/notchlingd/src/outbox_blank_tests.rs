use crate::acting::Acting;
use crate::acting_ops_tests::{app, code, sessions, FakeRoom};
use crate::op_group::OpGroup;
use crate::outbox_ops::OutboxOps;
use notchling_engine::outbox::OutboxStore;
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, ErrorCode};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

fn rig() -> (OutboxOps, Arc<OutboxStore>) {
    let store = Arc::new(OutboxStore::in_memory_for_tests());
    let room = Arc::new(FakeRoom::default());
    room.app.store(true, Ordering::SeqCst);
    let roots = Roots {
        main_dir: PathBuf::from("/nonexistent/claude"),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let ops = OutboxOps::new(
        store.clone(),
        Arc::new(Mutex::new(sessions())),
        roots,
        Arc::new(Acting::in_memory(true)),
        room,
        None,
    );
    (ops, store)
}

#[test]
fn a_message_of_only_dropped_characters_is_refused_like_an_empty_one() {
    let (ops, store) = rig();
    for text in [
        "\u{200b}",
        "\u{feff}\u{2060}\n",
        " \u{3164}\u{115f} ",
        "\u{200d}\u{200c}\u{200e}",
        "\u{7}\u{1b}",
    ] {
        let sent = ops.call(
            &app(),
            ops::SESSION_SEND,
            &json!({"id": "job-idle", "text": text}),
        );
        assert_eq!(
            code(sent),
            (ErrorCode::BadRequest, "Type a message first.".into()),
            "{text:?}"
        );
    }
    assert!(store.list(None).unwrap().is_empty());
}

#[test]
fn a_message_with_one_visible_character_among_dropped_ones_is_saved() {
    let (ops, store) = rig();
    for text in ["\u{200b}.\u{200b}", "\u{feff}\u{2800}", "\u{200b}\u{fffd}"] {
        let sent = ops.call(
            &app(),
            ops::SESSION_SEND,
            &json!({"id": "job-idle", "text": text}),
        );
        assert!(sent.is_ok(), "{text:?}");
    }
    let rows = store.list(Some("idle0001")).unwrap();
    assert!(
        rows[0].text.starts_with("\u{200b}.\u{200b}"),
        "the saved row keeps the text as typed"
    );
}
