use super::inbox::wire_item;
use super::model::{Control, SessionState};
use super::test_support::*;
use crate::inbox::{ActionId, Kind, State};
use serde_json::{json, Value};

fn events(io: &RecordingIo, from: usize) -> Vec<Value> {
    io.texts()[from..]
        .iter()
        .map(|t| serde_json::from_str(t).expect("json"))
        .collect()
}

#[test]
fn phone_lists_inbox() {
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.inbox = vec![
            inbox_item("a", Kind::SecretApproval),
            inbox_item("b", Kind::LimitNotice),
        ];
        s.inbox[1].state = State::Answered;
    });
    let (connection, io, _) = rig.login("test-phone");
    rig.send(&connection, r#"{"id":5,"op":"inbox.list"}"#);
    let reply = io.last_json();
    assert!(is_ok(&reply));
    assert_eq!(reply["id"], json!(5));
    let list = data(&reply);
    assert_eq!(list["unread"], json!(1));
    let ids: Vec<_> = list["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["id"].clone())
        .collect();
    assert_eq!(ids, [json!("a"), json!("b")]);
    assert_eq!(list["items"][0]["actions"][0]["id"], json!("allow_once"));
    assert_eq!(list["items"][0]["actions"][0]["primary"], json!(true));
    assert!(list["items"][0].get("open").is_none());
}

#[test]
fn phone_answers_an_open_item() {
    let rig = Rig::new();
    rig.backend
        .with(|s| s.inbox = vec![inbox_item("a", Kind::SecretApproval)]);
    let (connection, io, _) = rig.login("test-phone");
    rig.send(
        &connection,
        r#"{"id":6,"op":"inbox.answer","itemId":"a","action":"allow_once"}"#,
    );
    assert_eq!(
        data(&io.last_json()),
        json!({"answered": true, "state": "answered", "answer": "allow_once"})
    );
    assert_eq!(
        rig.backend.with(|s| s.inbox_answers.clone()),
        [("a".to_string(), ActionId::AllowOnce)]
    );
    assert!(rig.audit.recent().iter().any(|e| e.op == "inbox.answer"));
}

#[test]
fn phone_answer_first_wins() {
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.inbox = vec![inbox_item("a", Kind::SecretApproval)];
        s.inbox[0].state = State::Answered;
        s.inbox[0].answer = Some(ActionId::Deny);
        s.inbox[0].answered_by = Some("mac".into());
    });
    let (connection, io, _) = rig.login("test-phone");
    rig.send(
        &connection,
        r#"{"id":7,"op":"inbox.answer","itemId":"a","action":"allow_once"}"#,
    );
    let reply = io.last_json();
    assert!(is_ok(&reply));
    assert_eq!(
        data(&reply),
        json!({"answered": false, "state": "answered", "answer": "deny"})
    );
    assert!(rig.backend.with(|s| s.inbox_answers.is_empty()));
}

#[test]
fn phone_cannot_open_the_secret_form() {
    let rig = Rig::new();
    rig.backend
        .with(|s| s.inbox = vec![inbox_item("r", Kind::SecretRequest)]);
    let (connection, io, _) = rig.login("test-phone");
    rig.send(
        &connection,
        r#"{"id":8,"op":"inbox.answer","itemId":"r","action":"open"}"#,
    );
    assert_eq!(error_code(&io.last_json()), Some("invalid"));
    rig.send(
        &connection,
        r#"{"id":9,"op":"inbox.answer","itemId":"r","action":"cancel"}"#,
    );
    assert_eq!(data(&io.last_json())["answered"], json!(true));
}

#[test]
fn a_bad_inbox_answer_is_refused() {
    let rig = Rig::new();
    rig.backend
        .with(|s| s.inbox = vec![inbox_item("a", Kind::SecretApproval)]);
    let (connection, io, _) = rig.login("test-phone");
    for (body, code) in [
        (r#"{"id":1,"op":"inbox.answer","action":"deny"}"#, "invalid"),
        (
            r#"{"id":2,"op":"inbox.answer","itemId":"a","action":"nope"}"#,
            "invalid",
        ),
        (
            r#"{"id":3,"op":"inbox.answer","itemId":"a","action":"done"}"#,
            "invalid",
        ),
        (
            r#"{"id":4,"op":"inbox.answer","itemId":"zzz","action":"deny"}"#,
            "not_found",
        ),
    ] {
        rig.send(&connection, body);
        assert_eq!(error_code(&io.last_json()), Some(code), "{body}");
    }
    assert!(rig.backend.with(|s| s.inbox_answers.is_empty()));
}

#[test]
fn inbox_is_pushed_to_subscribers() {
    let rig = Rig::new();
    rig.backend
        .with(|s| s.inbox = vec![inbox_item("a", Kind::SecretApproval)]);
    let (connection, io, _) = rig.login("test-phone");
    rig.send(&connection, r#"{"id":2,"op":"subscribe","inbox":true}"#);
    assert!(is_ok(&io.last_json()));
    let mark = io.texts().len();

    rig.hub.tick(rig.backend.as_ref());
    let first = events(&io, mark);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["event"], json!("inbox"));
    assert_eq!(first[0]["item"]["id"], json!("a"));

    let mark = io.texts().len();
    rig.hub.tick(rig.backend.as_ref());
    assert_eq!(io.texts().len(), mark, "nothing changed, nothing pushed");

    rig.backend.with(|s| {
        s.inbox[0].state = State::Answered;
        s.inbox[0].answer = Some(ActionId::Deny);
        s.inbox.push(inbox_item("b", Kind::Notice));
    });
    rig.hub.tick(rig.backend.as_ref());
    let changed = events(&io, mark);
    let ids: Vec<_> = changed.iter().map(|e| e["item"]["id"].clone()).collect();
    assert_eq!(ids, [json!("a"), json!("b")]);
    assert_eq!(changed[0]["item"]["state"], json!("answered"));

    let mark = io.texts().len();
    rig.backend.with(|s| s.inbox.remove(0));
    rig.hub.tick(rig.backend.as_ref());
    let removed = events(&io, mark);
    assert_eq!(removed, [json!({"event": "inbox", "removed": "a"})]);
}

#[test]
fn old_phone_without_inbox_is_unaffected() {
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.sessions = vec![session("s1", Control::ConsolePty, SessionState::Running)];
        s.inbox = vec![inbox_item("a", Kind::SecretApproval)];
    });
    let (connection, io, _) = rig.login("old-phone");
    rig.send(&connection, r#"{"id":2,"op":"subscribe","sessions":true}"#);
    assert!(is_ok(&io.last_json()));
    let mark = io.texts().len();
    rig.backend.with(|s| {
        s.inbox.push(inbox_item("b", Kind::Notice));
        s.inbox[0].state = State::Expired;
    });
    rig.hub.tick(rig.backend.as_ref());
    rig.backend.with(|s| s.inbox.clear());
    rig.hub.tick(rig.backend.as_ref());
    assert_eq!(
        io.texts().len(),
        mark,
        "no inbox frame reaches an old phone"
    );
    for line in io.texts() {
        assert!(!line.contains("\"inbox\""), "{line}");
    }
    rig.send(&connection, r#"{"id":3,"op":"sessions.list"}"#);
    assert!(is_ok(&io.last_json()));
}

#[test]
fn unsubscribe_stops_inbox_pushes() {
    let rig = Rig::new();
    let (connection, io, _) = rig.login("test-phone");
    rig.send(&connection, r#"{"id":2,"op":"subscribe","inbox":true}"#);
    rig.send(&connection, r#"{"id":3,"op":"unsubscribe"}"#);
    let mark = io.texts().len();
    rig.backend
        .with(|s| s.inbox = vec![inbox_item("a", Kind::Notice)]);
    rig.hub.tick(rig.backend.as_ref());
    assert_eq!(io.texts().len(), mark);
}

#[test]
fn the_wire_item_leaves_out_what_the_phone_never_uses() {
    let mut item = inbox_item("r", Kind::SecretRequest);
    item.open = Some(json!({"form": "secret"}));
    item.answered_by = Some("mac".into());
    let wire = serde_json::to_value(wire_item(&item)).expect("json");
    assert!(wire.get("open").is_none());
    assert_eq!(wire["answeredBy"], json!("mac"));
    assert_eq!(wire["kind"], json!("secret_request"));
    assert_eq!(wire["createdMs"], json!(item.created_ms));
}
