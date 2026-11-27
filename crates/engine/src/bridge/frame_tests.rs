use super::model::*;
use super::router::MAX_MESSAGE_BYTES;
use super::test_support::*;
use notchling_protocol::generated::ChatRole;
use serde_json::{json, Value};

fn rig_with(
    chat: Vec<ChatMessage>,
) -> (
    Rig,
    std::sync::Arc<RecordingIo>,
    std::sync::Arc<super::Connection>,
) {
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.sessions = vec![session("s1", Control::ConsolePty, SessionState::Running)];
        s.chat = chat;
    });
    let (connection, io, _) = rig.login("test-phone");
    (rig, io, connection)
}

fn ask(
    rig: &Rig,
    io: &RecordingIo,
    connection: &std::sync::Arc<super::Connection>,
    body: Value,
) -> (usize, Value) {
    rig.send(connection, &body.to_string());
    let text = io.texts().last().cloned().expect("an answer");
    (text.len(), serde_json::from_str(&text).expect("json"))
}

fn long(n: usize, characters: usize, character: &str) -> ChatMessage {
    message(n, &character.repeat(characters), ChatRole::Assistant)
}

#[test]
fn a_page_of_many_long_messages_stays_under_the_frame_cap_and_sets_has_more() {
    let chat: Vec<_> = (0..30).map(|n| long(n, 90_000, "m")).collect();
    let newest = chat[29].id.clone();
    let (rig, io, connection) = rig_with(chat);
    let (size, answer) = ask(
        &rig,
        &io,
        &connection,
        json!({"id":2,"op":"chat.get","sessionId":"s1","limit":200}),
    );
    assert!(size <= MAX_MESSAGE_BYTES, "{size} bytes");
    let page = data(&answer);
    let kept = page["messages"].as_array().expect("messages");
    assert!(!kept.is_empty());
    assert_eq!(kept.last().map(|m| m["id"].clone()), Some(json!(newest)));
    assert_eq!(page["hasMore"], true);
    assert!(kept
        .iter()
        .all(|m| m["text"].as_str().map(str::len) == Some(90_000)));
}

#[test]
fn a_page_that_fits_is_left_alone() {
    let chat: Vec<_> = (0..10).map(|n| long(n, 1_000, "k")).collect();
    let (rig, io, connection) = rig_with(chat);
    let (_, answer) = ask(
        &rig,
        &io,
        &connection,
        json!({"id":2,"op":"chat.get","sessionId":"s1"}),
    );
    let page = data(&answer);
    assert_eq!(page["messages"].as_array().map(Vec::len), Some(10));
    assert_eq!(page["hasMore"], false);
}

#[test]
fn an_older_page_keeps_the_newest_that_fit_and_says_more_remain() {
    let chat: Vec<_> = (0..30).map(|n| long(n, 90_000, "o")).collect();
    let before = chat[25].id.clone();
    let expect_last = chat[24].id.clone();
    let (rig, io, connection) = rig_with(chat);
    let (size, answer) = ask(
        &rig,
        &io,
        &connection,
        json!({"id":2,"op":"chat.get","sessionId":"s1","limit":200,"before":before}),
    );
    assert!(size <= MAX_MESSAGE_BYTES);
    let page = data(&answer);
    let kept = page["messages"].as_array().expect("messages");
    assert_eq!(
        kept.last().map(|m| m["id"].clone()),
        Some(json!(expect_last))
    );
    assert_eq!(page["hasMore"], true);
}

#[test]
fn one_message_too_big_for_a_frame_is_shortened_with_the_note() {
    let (rig, io, connection) = rig_with(vec![long(1, 100_000, "日")]);
    let (size, answer) = ask(
        &rig,
        &io,
        &connection,
        json!({"id":2,"op":"chat.get","sessionId":"s1"}),
    );
    assert!(size <= MAX_MESSAGE_BYTES, "{size} bytes");
    let page = data(&answer);
    let text = page["messages"][0]["text"].as_str().expect("text");
    assert!(text.ends_with('…'));
    assert!(text.chars().count() > 80_000);
    assert_eq!(page["hasMore"], false);
}

#[test]
fn new_messages_pushed_to_a_phone_come_in_frames_under_the_cap() {
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.sessions = vec![session("s1", Control::ConsolePty, SessionState::Running)];
        s.chat = vec![message(0, "old", ChatRole::User)];
    });
    let (connection, io, _) = rig.login("test-phone");
    rig.send(
        &connection,
        r#"{"id":1,"op":"subscribe","sessions":true,"sessionId":"s1"}"#,
    );
    let baseline = io.texts().len();
    rig.backend.with(|s| {
        s.chat.extend((1..=6).map(|n| long(n, 90_000, "p")));
    });
    rig.hub.tick(rig.backend.as_ref());
    let pushed: Vec<String> = io.texts()[baseline..].to_vec();
    assert!(pushed.len() >= 2, "{} frames", pushed.len());
    assert!(pushed.iter().all(|t| t.len() <= MAX_MESSAGE_BYTES));
    let ids: Vec<String> = pushed
        .iter()
        .filter_map(|t| serde_json::from_str::<Value>(t).ok())
        .filter(|v| v["event"] == "chat")
        .flat_map(|v| v["messages"].as_array().cloned().unwrap_or_default())
        .map(|m| m["id"].as_str().expect("id").to_string())
        .collect();
    assert_eq!(
        ids,
        ["M-0001", "M-0002", "M-0003", "M-0004", "M-0005", "M-0006"]
    );
}
