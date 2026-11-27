use crate::bridge_backend::DaemonSources;
use crate::every_session_tests::{claude_list, every, every_with_agy, sources, Fixture, AGY_ID};
use notchling_engine::bridge::backend::BridgeBackend;
use notchling_engine::bridge::live::{ActingOff, LiveBackend};
use notchling_engine::bridge::model::{Session, SessionState};
use serde_json::{json, Value};

const ROW: &str = "agy:c0ffee00-1111-4222-8333-444455556666";

fn backend(fixture: &Fixture) -> LiveBackend<DaemonSources, ActingOff> {
    LiveBackend::new(
        sources(fixture.dir.root(), claude_list(), every(fixture)),
        ActingOff,
    )
}

fn agy_session(backend: &impl BridgeBackend) -> Session {
    backend
        .sessions()
        .into_iter()
        .find(|s| s.id == ROW)
        .expect("the Antigravity session is listed")
}

pub(crate) fn user(text: &str) -> Value {
    json!({"type": "USER_INPUT", "status": "DONE", "created_at": "2026-11-14T10:00:00Z",
           "content": format!("<USER_REQUEST>\n{text}\n</USER_REQUEST>")})
}

pub(crate) fn reply(text: &str) -> Value {
    json!({"type": "PLANNER_RESPONSE", "status": "DONE", "created_at": "2026-11-14T10:00:05Z",
           "content": text})
}

#[test]
fn a_running_antigravity_session_shows_running_on_the_phone() {
    let fixture = Fixture::new("agy-phone-running");
    fixture.antigravity(AGY_ID, "Busy one", "RUNNING", 30);
    assert_eq!(agy_session(&backend(&fixture)).state, SessionState::Running);
}

#[test]
fn an_idle_antigravity_session_shows_done() {
    let fixture = Fixture::new("agy-phone-idle");
    fixture.antigravity(AGY_ID, "Quiet one", "IDLE", 30);
    assert_eq!(agy_session(&backend(&fixture)).state, SessionState::Done);
}

#[test]
fn an_antigravity_chat_reaches_the_phone_from_its_log() {
    let fixture = Fixture::new("agy-phone-chat");
    fixture.antigravity(AGY_ID, "Chatty", "IDLE", 30);
    fixture.agy_log(AGY_ID, &[user("fix the bug"), reply("All done.")]);
    let backend = backend(&fixture);
    let messages = backend.chat_messages(ROW, None).unwrap();
    let seen: Vec<(String, &str)> = messages
        .iter()
        .map(|m| (format!("{:?}", m.role), m.text.as_str()))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("User".to_string(), "fix the bug"),
            ("Assistant".to_string(), "All done.")
        ]
    );
    let listed = agy_session(&backend);
    assert_eq!(listed.last_message.unwrap().text, "All done.");
}

fn bare(id: &str) -> String {
    id.to_string()
}

#[test]
fn a_long_log_is_cut_to_the_newest_512_kb() {
    let fixture = Fixture::new("agy-phone-long");
    fixture.antigravity(AGY_ID, "Long", "IDLE", 30);
    let filler = "x".repeat(2_000);
    let mut steps = vec![user("start")];
    for n in 0..600 {
        steps.push(reply(&format!("{n:04} {filler}")));
    }
    steps.push(reply("the newest reply"));
    fixture.agy_log(AGY_ID, &steps);
    let (_, agy) = every_with_agy(&fixture);
    let read = agy.phone_read(&bare(AGY_ID), false);
    assert!(read.omitted > 0, "the oldest steps are left out");
    let size = serde_json::to_vec(&read.conversation).unwrap().len();
    assert!(size <= 512 * 1024, "{size}");
    let last = read.conversation.items.last().unwrap();
    assert!(format!("{:?}", last.kind).contains("the newest reply"));
    assert!(!format!("{:?}", read.conversation.items.first().unwrap().kind).contains("start"));
}

#[test]
fn an_unchanged_log_is_read_once_and_a_grown_one_again() {
    let fixture = Fixture::new("agy-phone-cache");
    fixture.antigravity(AGY_ID, "Cached", "IDLE", 30);
    fixture.agy_log(AGY_ID, &[user("one"), reply("first")]);
    let (_, agy) = every_with_agy(&fixture);
    for _ in 0..3 {
        agy.phone_read(&bare(AGY_ID), false);
    }
    assert_eq!(agy.log_reads(), 1);
    fixture.agy_log(
        AGY_ID,
        &[user("one"), reply("first"), user("two"), reply("second")],
    );
    let read = agy.phone_read(&bare(AGY_ID), false);
    assert_eq!(agy.log_reads(), 2);
    assert_eq!(read.conversation.items.len(), 4);
}

#[test]
fn the_row_says_whether_a_closed_session_works() {
    let fixture = Fixture::new("agy-phone-row");
    fixture.antigravity(AGY_ID, "Row", "IDLE", 30);
    let (_, agy) = every_with_agy(&fixture);
    assert!(agy.phone_read(&bare(AGY_ID), true).working);
    assert!(!agy.phone_read(&bare(AGY_ID), false).working);
}

#[test]
fn a_list_reads_only_recent_logs_and_a_single_chat_reads_any() {
    let fixture = Fixture::new("agy-phone-old");
    let old = "c0ffee00-1111-4222-8333-444455557777";
    fixture.antigravity(AGY_ID, "New", "IDLE", 60);
    fixture.antigravity(old, "Old", "IDLE", 10 * 3600);
    fixture.agy_log(AGY_ID, &[user("new"), reply("fresh")]);
    fixture.agy_log(old, &[user("old"), reply("stale")]);
    let (every, agy) = every_with_agy(&fixture);
    let backend = LiveBackend::new(sources(fixture.dir.root(), claude_list(), every), ActingOff);
    assert_eq!(backend.sessions().len(), 3);
    assert_eq!(
        agy.log_reads(),
        1,
        "the ten hour old log is not read for a list"
    );
    let messages = backend.chat_messages(&format!("agy:{old}"), None).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(agy.log_reads(), 2);
}

#[test]
fn a_session_with_no_log_has_an_empty_chat() {
    let fixture = Fixture::new("agy-phone-nolog");
    fixture.antigravity(AGY_ID, "No log", "IDLE", 30);
    assert!(backend(&fixture)
        .chat_messages(ROW, None)
        .unwrap()
        .is_empty());
}
