use crate::acting_ops_tests::{app, code};
use crate::codex_ops::tests::{self as codex_t, SESSION as CODEX_SESSION};
use crate::event_assert::of;
use crate::op_group::OpGroup;
use crate::outbox_ops::OutboxOps;
use crate::outbox_runners::link_codex;
use crate::outbox_testkit::{rig, rig_with, Options, Rig, DIALOG, DRAFT, IDLE, T0};
use notchling_engine::outbox::{New, OutboxStore, State, Tool, Verdict, Via, REPLY_WAIT_MS};
use notchling_engine::reply::exec::Failure;
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, ApiError, ErrorCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn reply(r: &Rig, text: &str) -> Result<Value, ApiError> {
    r.acting.call(
        &app(),
        ops::SESSION_REPLY,
        &json!({"id": "job-idle", "text": text}),
    )
}

fn reply_in_thread(r: &Rig, text: &'static str) -> Receiver<Result<Value, ApiError>> {
    let (tx, rx) = channel();
    let acting = r.acting.clone();
    std::thread::spawn(move || {
        let out = acting.call(
            &app(),
            ops::SESSION_REPLY,
            &json!({"id": "job-idle", "text": text}),
        );
        let _ = tx.send(out);
    });
    rx
}

fn until(what: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if what() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn rows(r: &Rig) -> Vec<notchling_engine::outbox::Row> {
    r.ops.store().list(Some(IDLE)).unwrap()
}

#[test]
fn reply_returns_sent_when_delivered() {
    let r = rig();
    r.threaded();
    assert_eq!(reply(&r, "hello there").unwrap(), json!({"sent": true}));
    assert!(r.typed().contains("hello there"));
    assert!(rows(&r).is_empty());
    r.postmen.stop();
}

#[test]
fn a_reply_types_the_plan_into_the_jobs_attach_terminal() {
    let r = rig();
    r.threaded();
    let out = r.acting.call(
        &app(),
        ops::SESSION_REPLY,
        &json!({"id": "job-idle", "text": "hi\x1b[201~\r1"}),
    );
    assert_eq!(out.unwrap(), json!({"sent": true}));
    assert_eq!(
        *r.attached.lock().unwrap(),
        vec![("idle0001".to_string(), Some("/lanes/b".to_string()))],
        "the lane's config folder goes with the job"
    );
    assert_eq!(
        *r.writes.lock().unwrap(),
        vec![
            notchling_engine::reply::keys::clear_input(),
            "\x1b[200~hi[201~\r1\x1b[201~".to_string(),
            "\r".to_string()
        ]
    );
    r.postmen.stop();
}

#[test]
fn reply_returns_queued_shape_after_50s() {
    let r = rig_with(Options {
        screen: DIALOG,
        ..Options::default()
    });
    r.threaded();
    let out = reply_in_thread(&r, "after the dialog");
    assert!(until(|| rows(&r)
        .first()
        .is_some_and(|row| row.state == State::Waiting)));
    assert!(out.recv_timeout(Duration::from_millis(300)).is_err());
    r.at(T0 + REPLY_WAIT_MS);
    let out = out.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    let row = rows(&r).remove(0);
    assert_eq!(
        out,
        json!({"sent": true, "state": "queued", "waiting": 1, "outboxId": row.id})
    );
    r.postmen.stop();
}

#[test]
fn reply_box_error_has_box_data_and_removes_row() {
    let r = rig_with(Options {
        screen: DRAFT,
        ..Options::default()
    });
    r.threaded();
    let err = reply(&r, "now fix it").unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert_eq!(err.message, Failure::NotCleared.to_string());
    assert_eq!(
        err.data,
        Some(json!({"why": "not_cleared", "box": "my own draft"}))
    );
    assert!(rows(&r).is_empty());
    let removed = r.room.named("outbox.removed");
    assert_eq!(removed.last().unwrap()["why"], "removed");
    r.postmen.stop();
}

#[test]
fn reply_unsure_error_removes_row() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    r.threaded();
    let err = reply(&r, "maybe in").unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert!(rows(&r).is_empty());
    r.postmen.stop();
}

#[test]
fn an_unsure_reply_says_it_may_already_be_in() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    r.threaded();
    let err = reply(&r, "maybe in").unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert_eq!(
        err.message,
        "It may already be in the session. Check the session before sending it again."
    );
    assert_ne!(err.message, Failure::NotSent.to_string());
    assert!(!err.message.contains("wasn't sent"));
    assert!(err.data.is_none());
    assert!(rows(&r).is_empty());
    r.postmen.stop();
}

#[test]
fn reply_returns_queued_shape_when_in_queue() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::InQueue);
    r.threaded();
    let started = Instant::now();
    let out = reply(&r, "in the queue").unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    let row = rows(&r).remove(0);
    assert_eq!(row.state, State::InQueue);
    assert_eq!(
        out,
        json!({"sent": true, "state": "queued", "waiting": 1, "outboxId": row.id})
    );
    r.postmen.stop();
}

#[test]
fn reply_to_a_stopped_session_fails_and_removes_the_row() {
    let r = rig();
    r.files.set_state(IDLE, "stopped");
    r.threaded();
    let err = reply(&r, "hello?").unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert_eq!(err.message, "The session is stopped.");
    assert!(rows(&r).is_empty());
    assert_eq!(
        r.room.named("outbox.removed").last().unwrap()["why"],
        "removed"
    );
    r.postmen.stop();
}

#[test]
fn reply_to_a_vanished_session_fails_and_removes_the_row() {
    let r = rig_with(Options {
        screen: DIALOG,
        ..Options::default()
    });
    r.threaded();
    let out = reply_in_thread(&r, "anyone there");
    assert!(until(|| rows(&r)
        .first()
        .is_some_and(|row| row.state == State::Waiting)));
    r.files.vanish(IDLE);
    r.at(T0 + 1_000);
    r.postmen.waker()(IDLE);
    std::thread::sleep(Duration::from_millis(1_200));
    r.at(T0 + 61_000);
    r.postmen.waker()(IDLE);
    let err = out
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert_eq!(err.message, "No such session.");
    assert!(rows(&r).is_empty());
    assert_eq!(
        r.room.named("outbox.removed").last().unwrap()["why"],
        "removed"
    );
    r.postmen.stop();
}

#[test]
fn reply_waiting_when_the_row_is_removed_fails_as_removed() {
    let r = rig_with(Options {
        screen: DIALOG,
        ..Options::default()
    });
    r.threaded();
    let out = reply_in_thread(&r, "never mind");
    assert!(until(|| rows(&r)
        .first()
        .is_some_and(|row| row.state == State::Waiting)));
    let id = rows(&r).remove(0).id;
    r.ops
        .call(&app(), ops::OUTBOX_REMOVE, &json!({"id": id}))
        .unwrap();
    let err = out
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert_eq!(err.message, "The message was removed.");
    r.postmen.stop();
}

#[test]
fn reply_waiting_when_its_session_is_discarded_fails_as_removed() {
    let r = rig_with(Options {
        screen: DIALOG,
        ..Options::default()
    });
    r.threaded();
    let out = reply_in_thread(&r, "never mind");
    assert!(until(|| rows(&r)
        .first()
        .is_some_and(|row| row.state == State::Waiting)));
    r.ops.discard_session(IDLE).unwrap();
    let err = out
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_err();
    assert_eq!(err.message, "The message was removed.");
    r.postmen.stop();
}

#[test]
fn held_discard_discards_session_rows() {
    let r = rig();
    r.send("job-idle", "one");
    r.send("job-idle", "two");
    let out = r
        .acting
        .call(
            &app(),
            ops::SESSION_HELD_DISCARD,
            &json!({"id": "job-idle"}),
        )
        .unwrap();
    assert_eq!(out, json!({"discarded": true}));
    assert!(rows(&r).is_empty());
    let again = r
        .acting
        .call(
            &app(),
            ops::SESSION_HELD_DISCARD,
            &json!({"id": "job-idle"}),
        )
        .unwrap();
    assert_eq!(again, json!({"discarded": false}));
}

#[test]
fn reply_refuses_sessions_that_take_no_rows() {
    let r = rig();
    let (c, m) = code(r.acting.call(
        &app(),
        ops::SESSION_REPLY,
        &json!({"id": "term", "text": "hi"}),
    ));
    assert_eq!(
        (c, m.as_str()),
        (ErrorCode::BadRequest, "Reply in its terminal.")
    );
    let (c, m) = code(r.acting.call(
        &app(),
        ops::SESSION_REPLY,
        &json!({"id": "nope", "text": "hi"}),
    ));
    assert_eq!((c, m.as_str()), (ErrorCode::BadRequest, "No such session."));
    assert!(r.ops.store().list(None).unwrap().is_empty());
    let (c, _) = code(r.acting.call(
        &app(),
        ops::SESSION_REPLY,
        &json!({"id": "job-idle", "text": "hi", "clear_box": "yes"}),
    ));
    assert_eq!(c, ErrorCode::BadRequest);
}

#[test]
fn session_remove_deletes_rows() {
    let r = rig();
    r.send("job-idle", "one");
    r.send("job-idle", "two");
    let out = r
        .acting
        .call(&app(), ops::SESSION_REMOVE, &json!({"id": "job-idle"}))
        .unwrap();
    assert_eq!(out, json!({"removed": true}));
    assert!(rows(&r).is_empty());
    let removed = r.room.named("outbox.removed");
    assert_eq!(removed.len(), 2);
    assert!(removed.iter().all(|e| e["why"] == "session_removed"));
}

#[test]
fn codex_delete_deletes_rows() {
    let s = codex_t::setup("outbox-delete", true);
    let roots = Roots {
        main_dir: PathBuf::from("/nonexistent/claude"),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let outbox = Arc::new(OutboxOps::new(
        Arc::new(OutboxStore::in_memory_for_tests()),
        Arc::new(Mutex::new(json!([]))),
        roots,
        s.acting.clone(),
        s.room.clone(),
        None,
    ));
    link_codex(&outbox, &s.ops);
    let key = format!("codex:{CODEX_SESSION}");
    let new = New {
        session: key.clone(),
        tool: Tool::Codex,
        text: "later".into(),
        options: None,
        via: Via::Chat,
        clear_box: false,
    };
    outbox.store().insert(&new, T0).unwrap();
    assert_eq!(
        s.call(ops::CODEX_DELETE, json!({"id": key})),
        Ok(json!({"deleted": true}))
    );
    assert!(outbox.store().list(Some(&key)).unwrap().is_empty());
    let removed = s.room.named("outbox.removed");
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0]["why"], "session_removed");
}

#[test]
fn nothing_publishes_session_held() {
    let r = rig_with(Options {
        screen: DRAFT,
        ..Options::default()
    });
    r.threaded();
    let _ = reply(&r, "into a box");
    r.show(crate::acting_ops_tests::PROMPT);
    reply(&r, "now it goes").unwrap();
    r.acting
        .call(
            &app(),
            ops::SESSION_HELD_DISCARD,
            &json!({"id": "job-idle"}),
        )
        .unwrap();
    r.postmen.stop();
    assert!(r.room.named("session.held").is_empty());
    assert!(of(&r.sink, "session.reply_held").is_empty());
    assert!(!of(&r.sink, "session.reply").is_empty());
}
