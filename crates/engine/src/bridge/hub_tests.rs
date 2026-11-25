use super::model::*;
use super::test_support::*;
use notchling_protocol::generated::ChatRole;
use serde_json::{json, Value};

fn events(io: &RecordingIo, from: usize) -> Vec<Value> {
    io.texts()[from..]
        .iter()
        .map(|t| serde_json::from_str(t).expect("json"))
        .collect()
}

#[test]
fn the_hub_pushes_only_changes() {
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.sessions = vec![session("s1", Control::ConsolePty, SessionState::Running)];
        s.chat = vec![message(1, "old", ChatRole::User)];
    });
    let (connection, io, _) = rig.login("test-phone");
    rig.send(
        &connection,
        r#"{"id":1,"op":"subscribe","sessions":true,"sessionId":"s1"}"#,
    );
    let baseline = io.texts().len();

    rig.hub.tick(rig.backend.as_ref());
    assert_eq!(
        io.texts().len(),
        baseline,
        "nothing changed, nothing pushed"
    );

    rig.backend.with(|s| {
        s.sessions = vec![
            session("s1", Control::ConsolePty, SessionState::Done),
            session("s2", Control::ConsolePty, SessionState::Running),
        ];
        s.chat.push(message(2, "new one", ChatRole::Assistant));
    });
    rig.hub.tick(rig.backend.as_ref());
    let pushed = events(&io, baseline);
    let names: Vec<_> = pushed.iter().map(|e| e["event"].clone()).collect();
    assert_eq!(names, [json!("sessions"), json!("chat")]);
    let chat = &pushed[1];
    assert_eq!(chat["messages"][0]["text"], "new one");
    assert_eq!(chat["messages"].as_array().map(Vec::len), Some(1));
    assert_eq!(chat["sessionId"], "s1");

    rig.hub.tick(rig.backend.as_ref());
    assert_eq!(
        io.texts().len(),
        baseline + 2,
        "a second tick with no change is silent"
    );
}

#[test]
fn the_hub_pushes_a_prompt_appearing_and_resolving() {
    let rig = Rig::new();
    rig.backend
        .with(|s| s.sessions = vec![session("s1", Control::ConsolePty, SessionState::Running)]);
    let (connection, io, _) = rig.login("test-phone");
    rig.send(&connection, r#"{"id":1,"op":"subscribe","sessions":true}"#);
    let baseline = io.texts().len();

    let mut waiting = session("s1", Control::ConsolePty, SessionState::NeedsInput);
    waiting.pending = Some(permission("r1", "s1"));
    rig.backend.with(|s| s.sessions = vec![waiting]);
    rig.hub.tick(rig.backend.as_ref());
    let pushed = events(&io, baseline);
    let names: Vec<_> = pushed.iter().map(|e| e["event"].clone()).collect();
    assert_eq!(names, [json!("sessions"), json!("prompt")]);
    assert_eq!(pushed[1]["sessionId"], "s1");

    let after = io.texts().len();
    rig.backend
        .with(|s| s.sessions = vec![session("s1", Control::ConsolePty, SessionState::Running)]);
    rig.hub.tick(rig.backend.as_ref());
    let pushed = events(&io, after);
    let names: Vec<_> = pushed.iter().map(|e| e["event"].clone()).collect();
    assert_eq!(names, [json!("sessions"), json!("prompt")]);
    assert_eq!(pushed[1]["resolved"], true);
    assert_eq!(pushed[1]["requestId"], "r1");
}

#[test]
fn open_goes_only_to_logged_in_phones_and_closed_connections_leave() {
    let rig = Rig::new();
    let (connection, io, _) = rig.login("test-phone");
    let (_stranger, stranger_io) = rig.connect("192.168.1.99");
    assert_eq!(rig.hub.notify_open(), 1);
    assert_eq!(io.last_json(), json!({"event": "open"}));
    assert!(stranger_io.texts().is_empty());

    rig.router.closed(&connection);
    assert_eq!(rig.hub.notify_open(), 0);
}
