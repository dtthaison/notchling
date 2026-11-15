use crate::acting::{Acting, ACTING_OFF};
use crate::acting_ops_tests::{app, code, row, sessions, FakeRoom, SESSION};
use crate::event_assert::{of, text};
use crate::late_emit::Events;
use crate::op_group::OpGroup;
use crate::outbox_ops::OutboxOps;
use notchling_engine::outbox::{OutboxStore, State, Why};
use notchling_engine::sessions::Roots;
use notchling_events::{MemorySink, Salt};
use notchling_local_api::{ops, topics, ApiError, ErrorCode, Tier};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const UUID_ROW: &str = "5b6c7d8e-1f2a-4b3c-8d4e-9f0a1b2c3d4e";

struct Rig {
    ops: OutboxOps,
    room: Arc<FakeRoom>,
    sink: Arc<MemorySink>,
    store: Arc<OutboxStore>,
    acting: Arc<Acting>,
}

fn list() -> Value {
    let mut all = sessions();
    all.as_array_mut().unwrap().push(row(
        UUID_ROW,
        json!({"job": "uuid0001"}),
        "completed",
        Some(&format!("/tmp/none/{SESSION}.jsonl")),
    ));
    all
}

fn rig() -> Rig {
    let store = Arc::new(OutboxStore::in_memory_for_tests());
    let room = Arc::new(FakeRoom::default());
    room.app.store(true, Ordering::SeqCst);
    let acting = Arc::new(Acting::in_memory(true));
    let sink = Arc::new(MemorySink::new());
    let roots = Roots {
        main_dir: PathBuf::from("/nonexistent/claude"),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let ops = OutboxOps::new(
        store.clone(),
        Arc::new(Mutex::new(list())),
        roots,
        acting.clone(),
        room.clone(),
        Some(Events::fixed(sink.clone(), Salt([1; 32]))),
    );
    Rig {
        ops,
        room,
        sink,
        store,
        acting,
    }
}

fn call(r: &Rig, op: &str, args: Value) -> Result<Value, ApiError> {
    r.ops.call(&app(), op, &args)
}

fn send(r: &Rig, id: &str, text: &str) -> Value {
    call(r, ops::SESSION_SEND, json!({"id": id, "text": text})).unwrap()
}

fn id_of(sent: &Value) -> String {
    sent["outboxId"].as_str().unwrap().to_string()
}

#[test]
fn send_saves_and_answers_at_once() {
    let r = rig();
    let started = Instant::now();
    let sent = send(&r, "job-idle", "hello there");
    assert!(started.elapsed() < Duration::from_millis(100));
    assert_eq!(sent["state"], "queued");
    assert_eq!(sent["position"], 1);
    assert_eq!(sent["session"], "idle0001");
    let rows = r.store.list(Some("idle0001")).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, State::Queued);
    assert_eq!(rows[0].id, id_of(&sent));
    let changed = r.room.named("outbox.changed");
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["row"]["id"], json!(rows[0].id));
    assert_eq!(changed[0]["row"]["text"], "hello there");
    assert_eq!(changed[0]["row"]["position"], 1);
    let second = send(&r, "job-idle", "and more");
    assert_eq!(second["position"], 2);
}

#[test]
fn send_wakes_the_session_postman() {
    let r = rig();
    let woken = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = woken.clone();
    r.ops
        .set_wake(Arc::new(move |s| seen.lock().unwrap().push(s.to_string())));
    send(&r, "job-idle", "hi");
    assert_eq!(*woken.lock().unwrap(), ["idle0001"]);
}

#[test]
fn send_by_session_id_keys_row_by_job() {
    let r = rig();
    let sent = send(&r, UUID_ROW, "by uuid");
    assert_eq!(sent["session"], "uuid0001");
    assert_eq!(r.store.list(Some("uuid0001")).unwrap().len(), 1);
    for key in [UUID_ROW, "uuid0001"] {
        let rows = call(&r, ops::OUTBOX_LIST, json!({"session": key})).unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 1, "{key}");
        assert_eq!(rows[0]["session"], "uuid0001");
    }
}

#[test]
fn codex_and_agy_ids_are_their_own_key() {
    let r = rig();
    let sent = send(&r, "codex:t1", "to codex");
    assert_eq!(sent["session"], "codex:t1");
    let sent = send(&r, "agy:c9", "to agy");
    assert_eq!(sent["session"], "agy:c9");
    let rows = call(&r, ops::OUTBOX_LIST, json!({})).unwrap();
    assert_eq!(rows[0]["tool"], "codex");
    assert_eq!(rows[1]["tool"], "antigravity");
}

#[test]
fn codex_and_agy_ids_need_a_valid_suffix() {
    let r = rig();
    for id in [
        "codex:",
        "codex:a/b",
        "codex:-x",
        "agy:",
        "agy:a/b",
        "agy:a\\b",
    ] {
        assert_eq!(
            code(call(&r, ops::SESSION_SEND, json!({"id": id, "text": "hi"}))),
            (ErrorCode::BadRequest, "No such session.".into()),
            "{id}"
        );
    }
    assert!(r.store.list(None).unwrap().is_empty());
}

#[test]
fn antigravity_alias_is_keyed_as_agy() {
    let r = rig();
    let sent = send(&r, "antigravity:c9", "to agy");
    assert_eq!(sent["session"], "agy:c9");
    send(&r, "agy:c9", "again");
    assert_eq!(r.store.list(Some("agy:c9")).unwrap().len(), 2);
    assert!(r.store.list(Some("antigravity:c9")).unwrap().is_empty());
}

#[test]
fn edit_into_a_menu_command_is_refused() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "hello"));
    assert_eq!(
        code(call(
            &r,
            ops::OUTBOX_EDIT,
            json!({"id": id, "text": "/mcp"})
        )),
        (
            ErrorCode::BadRequest,
            "That command opens a menu; open the session's terminal.".into()
        )
    );
    assert_eq!(r.store.get(&id).unwrap().unwrap().text, "hello");
    let codex = id_of(&send(&r, "codex:t1", "hello"));
    call(&r, ops::OUTBOX_EDIT, json!({"id": codex, "text": "/mcp"})).unwrap();
}

#[test]
fn busy_refusal_is_worded_from_the_row_as_it_is_now() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "hello"));
    r.store.take(&id, 10).unwrap().unwrap();
    assert_eq!(
        code(Err(r.ops.busy_refusal(&id))),
        (ErrorCode::Failed, "It's being sent.".into())
    );
    r.store
        .set_state(&id, &[State::Sending], State::InQueue, None, None, 11)
        .unwrap();
    assert_eq!(
        code(Err(r.ops.busy_refusal(&id))),
        (ErrorCode::Failed, "It's in the session's queue.".into())
    );
    r.store.delete(&id).unwrap();
    assert_eq!(
        code(Err(r.ops.busy_refusal(&id))),
        (ErrorCode::BadRequest, "No such message.".into())
    );
}

#[test]
fn send_refuses_with_exact_words() {
    let r = rig();
    let refuse = |args: Value| code(call(&r, ops::SESSION_SEND, args));
    let bad = ErrorCode::BadRequest;
    assert_eq!(
        refuse(json!({"id": "job-idle", "text": "  "})),
        (bad, "Type a message first.".into())
    );
    let long = "x".repeat(100 * 1024 + 1);
    assert_eq!(
        refuse(json!({"id": "job-idle", "text": long})),
        (bad, "That message is over 100 KB.".into())
    );
    assert_eq!(
        refuse(json!({"id": "nope", "text": "hi"})),
        (bad, "No such session.".into())
    );
    assert_eq!(
        refuse(json!({"id": "term", "text": "hi"})),
        (bad, "Reply in its terminal.".into())
    );
    assert_eq!(
        refuse(json!({"id": "job-idle", "text": "/mcp"})),
        (
            bad,
            "That command opens a menu; open the session's terminal.".into()
        )
    );
    for draft in ["codex-new:abc", "agy-new:abc"] {
        assert_eq!(
            refuse(json!({"id": draft, "text": "hi"})),
            (bad, "Start the session with codex.send or agy.send.".into())
        );
    }
    assert!(r.store.list(None).unwrap().is_empty());
    for n in 0..200 {
        send(&r, "job-idle", &format!("m{n}"));
    }
    assert_eq!(
        refuse(json!({"id": "job-idle", "text": "one more"})),
        (
            ErrorCode::Failed,
            "200 messages are already waiting for this session.".into()
        )
    );
    assert_eq!(r.store.list(None).unwrap().len(), 200);
}

#[test]
fn send_says_it_could_not_save_when_the_store_is_unavailable() {
    let r = rig();
    let ops = r.ops.without_store();
    let result = ops.call(
        &app(),
        ops::SESSION_SEND,
        &json!({"id": "job-idle", "text": "hi"}),
    );
    assert_eq!(
        code(result),
        (ErrorCode::Failed, "Couldn't save the message.".into())
    );
}

#[test]
fn send_refused_while_acting_off() {
    let r = rig();
    r.acting.set(false);
    for (op, args) in [
        (ops::SESSION_SEND, json!({"id": "job-idle", "text": "hi"})),
        (ops::OUTBOX_EDIT, json!({"id": "x", "text": "hi"})),
        (ops::OUTBOX_REMOVE, json!({"id": "x"})),
        (ops::OUTBOX_RESEND, json!({"id": "x"})),
        (ops::OUTBOX_DISCARD, json!({"session": "job-idle"})),
    ] {
        assert_eq!(
            code(call(&r, op, args)),
            (ErrorCode::Forbidden, ACTING_OFF.into()),
            "{op}"
        );
    }
    assert!(r.store.list(None).unwrap().is_empty());
}

#[test]
fn list_works_while_acting_off() {
    let r = rig();
    send(&r, "job-idle", "hi");
    r.acting.set(false);
    let rows = call(&r, ops::OUTBOX_LIST, json!({})).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(
        r.ops.spec(ops::OUTBOX_LIST),
        Some(notchling_local_api::OpSpec::APP)
    );
}

#[test]
fn specs_are_app_tier() {
    let r = rig();
    for op in [
        ops::SESSION_SEND,
        ops::OUTBOX_LIST,
        ops::OUTBOX_EDIT,
        ops::OUTBOX_REMOVE,
        ops::OUTBOX_RESEND,
        ops::OUTBOX_DISCARD,
    ] {
        assert_eq!(
            r.ops.spec(op),
            Some(notchling_local_api::OpSpec::APP),
            "{op}"
        );
    }
    assert_eq!(r.ops.spec(ops::SESSION_REPLY), None);
}

#[test]
fn edit_changes_a_queued_row_and_publishes() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "first"));
    let done = call(&r, ops::OUTBOX_EDIT, json!({"id": id, "text": "second"})).unwrap();
    assert_eq!(done, json!({"edited": true}));
    assert_eq!(r.store.get(&id).unwrap().unwrap().text, "second");
    assert_eq!(r.room.named("outbox.changed").len(), 2);
    let edited = of(&r.sink, "session.outbox");
    assert_eq!(text(&edited[1], "then").as_deref(), Some("edited"));
    assert_eq!(text(&edited[1], "chars").as_deref(), Some("6"));
    assert_eq!(
        code(call(&r, ops::OUTBOX_EDIT, json!({"id": id, "text": " "}))),
        (ErrorCode::BadRequest, "Type a message first.".into())
    );
    assert_eq!(
        code(call(
            &r,
            ops::OUTBOX_EDIT,
            json!({"id": "nope", "text": "x"})
        ))
        .0,
        ErrorCode::BadRequest
    );
}

#[test]
fn edit_on_sending_row_fails_being_sent() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "hello"));
    r.store.take(&id, 10).unwrap().unwrap();
    let before = r.room.named("outbox.changed").len();
    assert_eq!(
        code(call(&r, ops::OUTBOX_EDIT, json!({"id": id, "text": "x"}))),
        (ErrorCode::Failed, "It's being sent.".into())
    );
    assert_eq!(
        code(call(&r, ops::OUTBOX_REMOVE, json!({"id": id}))),
        (ErrorCode::Failed, "It's being sent.".into())
    );
    assert_eq!(r.store.get(&id).unwrap().unwrap().text, "hello");
    assert_eq!(r.room.named("outbox.changed").len(), before);
}

#[test]
fn edit_and_remove_on_in_queue_row_fail_in_queue() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "hello"));
    r.store.take(&id, 10).unwrap().unwrap();
    r.store
        .set_state(&id, &[State::Sending], State::InQueue, None, None, 11)
        .unwrap();
    let published = r.room.events.lock().unwrap().len();
    let events = r.sink.events().len();
    for (op, args) in [
        (ops::OUTBOX_EDIT, json!({"id": id, "text": "x"})),
        (ops::OUTBOX_REMOVE, json!({"id": id})),
    ] {
        assert_eq!(
            code(call(&r, op, args)),
            (ErrorCode::Failed, "It's in the session's queue.".into()),
            "{op}"
        );
    }
    let row = r.store.get(&id).unwrap().unwrap();
    assert_eq!((row.state, row.text.as_str()), (State::InQueue, "hello"));
    assert_eq!(r.room.events.lock().unwrap().len(), published);
    assert_eq!(r.sink.events().len(), events);
}

#[test]
fn remove_drops_unsure_row() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "hello"));
    r.store.take(&id, 10).unwrap().unwrap();
    r.store
        .set_state(
            &id,
            &[State::Sending],
            State::Unsure,
            Some(Why::NotFound),
            None,
            11,
        )
        .unwrap();
    let done = call(&r, ops::OUTBOX_REMOVE, json!({"id": id})).unwrap();
    assert_eq!(done, json!({"removed": true}));
    assert!(r.store.list(None).unwrap().is_empty());
    let gone = r.room.named("outbox.removed");
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0]["id"], json!(id));
    assert_eq!(gone[0]["session"], "idle0001");
    assert_eq!(gone[0]["why"], "removed");
    let kept = of(&r.sink, "session.outbox");
    assert_eq!(
        text(kept.last().unwrap(), "then").as_deref(),
        Some("removed")
    );
}

#[test]
fn resend_moves_unsure_to_queued() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "hello"));
    r.store.take(&id, 10).unwrap().unwrap();
    r.store
        .set_state(
            &id,
            &[State::Sending],
            State::Unsure,
            Some(Why::NotFound),
            None,
            11,
        )
        .unwrap();
    let woken = Arc::new(Mutex::new(0));
    let seen = woken.clone();
    r.ops
        .set_wake(Arc::new(move |_| *seen.lock().unwrap() += 1));
    let done = call(&r, ops::OUTBOX_RESEND, json!({"id": id})).unwrap();
    assert_eq!(done, json!({"state": "queued"}));
    let row = r.store.get(&id).unwrap().unwrap();
    assert_eq!((row.state, row.why), (State::Queued, None));
    assert_eq!(*woken.lock().unwrap(), 1);
    assert_eq!(
        r.room.named("outbox.changed").last().unwrap()["row"]["state"],
        "queued"
    );
    assert_eq!(
        code(call(&r, ops::OUTBOX_RESEND, json!({"id": id}))).0,
        ErrorCode::BadRequest
    );
}

#[test]
fn resend_box_needs_clear_box() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "hello"));
    r.store.take(&id, 10).unwrap().unwrap();
    r.store
        .set_state(
            &id,
            &[State::Sending],
            State::Box,
            Some(Why::NotCleared),
            None,
            11,
        )
        .unwrap();
    assert_eq!(
        code(call(&r, ops::OUTBOX_RESEND, json!({"id": id}))).0,
        ErrorCode::BadRequest
    );
    assert_eq!(r.store.get(&id).unwrap().unwrap().state, State::Box);
    let done = call(&r, ops::OUTBOX_RESEND, json!({"id": id, "clear_box": true})).unwrap();
    assert_eq!(done, json!({"state": "queued"}));
    let row = r.store.get(&id).unwrap().unwrap();
    assert_eq!((row.state, row.clear_box), (State::Queued, true));
}

#[test]
fn resend_try_now_on_a_retry_wait() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "hello"));
    r.store
        .set_state(
            &id,
            &[State::Queued],
            State::Waiting,
            Some(Why::Retry),
            Some(i64::MAX),
            11,
        )
        .unwrap();
    let done = call(&r, ops::OUTBOX_RESEND, json!({"id": id})).unwrap();
    assert_eq!(done, json!({"state": "queued"}));
    let row = r.store.get(&id).unwrap().unwrap();
    assert_eq!((row.state, row.next_ms), (State::Queued, None));
    r.store
        .set_state(
            &id,
            &[State::Queued],
            State::Waiting,
            Some(Why::Dialog),
            None,
            12,
        )
        .unwrap();
    assert_eq!(
        code(call(&r, ops::OUTBOX_RESEND, json!({"id": id}))).0,
        ErrorCode::BadRequest
    );
}

#[test]
fn discard_removes_all_but_sending() {
    let r = rig();
    let ids: Vec<String> = (0..4)
        .map(|n| id_of(&send(&r, "job-idle", &format!("m{n}"))))
        .collect();
    send(&r, "job-a", "other session");
    r.store.take(&ids[0], 10).unwrap().unwrap();
    r.store
        .set_state(&ids[0], &[State::Sending], State::InQueue, None, None, 11)
        .unwrap();
    r.store.take(&ids[1], 12).unwrap_or(None);
    r.store
        .set_state(
            &ids[2],
            &[State::Queued],
            State::Unsure,
            Some(Why::Dropped),
            None,
            13,
        )
        .unwrap();
    r.store.take(&ids[3], 14).unwrap_or(None);
    let live = r.store.sending().unwrap();
    let done = call(&r, ops::OUTBOX_DISCARD, json!({"session": "job-idle"})).unwrap();
    let left = r.store.list(Some("idle0001")).unwrap();
    assert_eq!(left.len(), live.len());
    assert!(left.iter().all(|row| row.state == State::Sending));
    assert_eq!(done, json!({"discarded": 4 - live.len()}));
    let gone = r.room.named("outbox.removed");
    assert_eq!(gone.len(), 4 - live.len());
    assert!(gone.iter().all(|g| g["why"] == "discarded"));
    assert_eq!(r.store.list(Some("abcd1234")).unwrap().len(), 1);
    let discarded = of(&r.sink, "session.outbox")
        .iter()
        .filter(|e| text(e, "then").as_deref() == Some("discarded"))
        .count();
    assert_eq!(discarded, 4 - live.len());
}

#[test]
fn discard_removes_an_in_queue_row() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "hello"));
    r.store.take(&id, 10).unwrap().unwrap();
    r.store
        .set_state(&id, &[State::Sending], State::InQueue, None, None, 11)
        .unwrap();
    let done = call(&r, ops::OUTBOX_DISCARD, json!({"session": "idle0001"})).unwrap();
    assert_eq!(done, json!({"discarded": 1}));
    assert!(r.store.list(None).unwrap().is_empty());
}

#[test]
fn send_clear_box_is_stored_on_row() {
    let r = rig();
    let sent = call(
        &r,
        ops::SESSION_SEND,
        json!({"id": "job-idle", "text": "hi", "clear_box": true, "via": "phone",
            "options": {"model": "m"}}),
    )
    .unwrap();
    let row = r.store.get(&id_of(&sent)).unwrap().unwrap();
    assert!(row.clear_box);
    assert_eq!(row.via.as_str(), "phone");
    assert_eq!(row.options, Some(json!({"model": "m"})));
}

#[test]
fn note_shows_on_wire_row_and_is_never_stored() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "hello"));
    r.store.take(&id, 10).unwrap().unwrap();
    r.store
        .set_state(
            &id,
            &[State::Sending],
            State::Box,
            Some(Why::NotCleared),
            None,
            11,
        )
        .unwrap();
    r.ops.set_note(&id, Some("typed by a person".into()));
    assert_eq!(r.ops.note(&id).as_deref(), Some("typed by a person"));
    let rows = call(&r, ops::OUTBOX_LIST, json!({})).unwrap();
    assert_eq!(rows[0]["box"], "typed by a person");
    assert!(rows[0].get("error").is_none());
    let reopened = OutboxStore::in_memory_for_tests();
    assert!(reopened.list(None).unwrap().is_empty());
    let stored = format!("{:?}", r.store.get(&id).unwrap().unwrap());
    assert!(!stored.contains("typed by a person"));
    call(&r, ops::OUTBOX_RESEND, json!({"id": id, "clear_box": true})).unwrap();
    assert_eq!(r.ops.note(&id), None);
}

#[test]
fn a_retry_row_carries_its_error() {
    let r = rig();
    let id = id_of(&send(&r, "codex:t1", "hello"));
    r.store
        .set_state(
            &id,
            &[State::Queued],
            State::Waiting,
            Some(Why::Retry),
            Some(5),
            11,
        )
        .unwrap();
    r.ops.set_note(&id, Some("runner said no".into()));
    let rows = call(&r, ops::OUTBOX_LIST, json!({})).unwrap();
    assert_eq!(rows[0]["error"], "runner said no");
    assert!(rows[0].get("box").is_none());
    assert_eq!(rows[0]["why"], "retry");
}

#[test]
fn resend_resume_without_hook_fails() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "hello"));
    r.store
        .set_state(&id, &[State::Queued], State::Stopped, None, None, 11)
        .unwrap();
    assert_eq!(
        code(call(
            &r,
            ops::OUTBOX_RESEND,
            json!({"id": id, "resume": true})
        )),
        (ErrorCode::Failed, "Resume isn't available.".into())
    );
    assert_eq!(r.store.get(&id).unwrap().unwrap().state, State::Stopped);
    assert_eq!(
        code(call(&r, ops::OUTBOX_RESEND, json!({"id": id}))).0,
        ErrorCode::BadRequest
    );
}

#[test]
fn resend_resume_calls_hook_then_queues() {
    let r = rig();
    let id = id_of(&send(&r, "job-idle", "hello"));
    r.store
        .set_state(&id, &[State::Queued], State::Stopped, None, None, 11)
        .unwrap();
    let seen = Arc::new(Mutex::new(Vec::<(String, State)>::new()));
    let hook = seen.clone();
    let store = r.store.clone();
    r.ops.set_resume(Arc::new(move |row| {
        let state = store.get(&row.id).unwrap().unwrap().state;
        hook.lock().unwrap().push((row.id.clone(), state));
        Ok(())
    }));
    let done = call(&r, ops::OUTBOX_RESEND, json!({"id": id, "resume": true})).unwrap();
    assert_eq!(done, json!({"state": "queued"}));
    assert_eq!(*seen.lock().unwrap(), [(id.clone(), State::Stopped)]);
    assert_eq!(r.store.get(&id).unwrap().unwrap().state, State::Queued);

    r.store
        .set_state(&id, &[State::Queued], State::Stopped, None, None, 12)
        .unwrap();
    r.ops.set_resume(Arc::new(|_| {
        Err(ApiError::new(ErrorCode::Failed, "It would not start."))
    }));
    assert_eq!(
        code(call(
            &r,
            ops::OUTBOX_RESEND,
            json!({"id": id, "resume": true})
        )),
        (ErrorCode::Failed, "It would not start.".into())
    );
    assert_eq!(r.store.get(&id).unwrap().unwrap().state, State::Stopped);
}

#[test]
fn outbox_topic_is_app_tier() {
    let r = rig();
    assert_eq!(r.ops.topics(), [topics::OUTBOX]);
    assert_eq!(r.ops.topic_tier(topics::OUTBOX), Some(Tier::App));
    assert_eq!(r.ops.topic_tier(topics::INBOX), None);
}

#[test]
fn no_text_in_events() {
    let r = rig();
    let token = "sk-planted-token-1234";
    let id = id_of(&send(&r, "job-idle", &format!("use {token} now")));
    call(
        &r,
        ops::OUTBOX_EDIT,
        json!({"id": id, "text": format!("again {token}")}),
    )
    .unwrap();
    call(&r, ops::OUTBOX_REMOVE, json!({"id": id})).unwrap();
    send(&r, "job-idle", token);
    call(&r, ops::OUTBOX_DISCARD, json!({"session": "job-idle"})).unwrap();
    let events = r.sink.events();
    assert!(events.len() >= 5, "{}", events.len());
    for event in &events {
        assert_eq!(event.spec.name, "session.outbox");
        assert!(!format!("{event:?}").contains(token));
        assert!(text(event, "chars").is_some());
    }
}
