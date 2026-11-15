use super::*;
use crate::sessions::testutil::TempDir;
use serde_json::json;
use std::path::{Path, PathBuf};

const VERSIONS: [&str; 2] = ["2.1.322", "2.1.325"];
const OURS: &str = "keep going with the tidy";

fn fixture(version: &str, name: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/queue")
        .join(version)
        .join(format!("{name}.jsonl"));
    path.to_string_lossy().into_owned()
}

fn state(version: &str, name: &str) -> QueueState {
    queue_state(&fixture(version, name)).unwrap()
}

fn each_version(name: &str, expected: &[&str]) {
    for version in VERSIONS {
        let got = state(version, name);
        assert_eq!(got.waiting, expected, "{version}/{name}");
        assert_eq!(got.unknown, 0, "{version}/{name}");
    }
}

fn message(text: &str) -> Message {
    Message::parse(text, |_| false)
}

fn enqueue(second: u32, content: &str) -> String {
    json!({"type":"queue-operation","operation":"enqueue","timestamp":format!("2026-11-15T10:00:{second:02}.000Z"),"sessionId":"s","content":content}).to_string()
}

fn raw(lines: &[String]) -> Option<QueueState> {
    let dir = TempDir::new("queue-state");
    let path = dir.write("t.jsonl", &lines.join("\n"));
    queue_state(&path.to_string_lossy())
}

#[test]
fn enqueued_entry_waits() {
    each_version("waiting", &[OURS]);
}

#[test]
fn dequeue_then_user_line_frees() {
    each_version("dequeued", &[]);
}

#[test]
fn queued_command_frees() {
    each_version("queued_command", &[]);
}

#[test]
fn remove_frees() {
    each_version("removed", &[]);
}

#[test]
fn pop_all_frees() {
    each_version("pop_all", &[]);
}

#[test]
fn later_turn_duration_frees() {
    each_version("turn_end", &[]);
}

#[test]
fn typed_prompt_without_dequeue_frees_all() {
    each_version("no_dequeue", &["after the prompt"]);
}

#[test]
fn task_notification_never_waits() {
    each_version("task_notification", &[]);
}

#[test]
fn notification_dequeued_first_keeps_the_prompt_waiting() {
    each_version("notification_first", &[OURS]);
}

#[test]
fn persons_own_entry_waits() {
    let got = raw(&[enqueue(1, "ls -la"), enqueue(2, "[Image #3]what is this")]).unwrap();
    assert_eq!(got.waiting, ["ls -la", "[Image #3]what is this"]);
}

#[test]
fn merged_dequeue_frees_both_entries() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/outbox/merged.jsonl");
    let got = queue_state(&path.to_string_lossy()).unwrap();
    assert!(got.waiting.is_empty());
}

#[test]
fn bash_entry_is_freed_by_its_bash_input_line() {
    let bash = json!({"type":"user","message":{"role":"user","content":"<bash-input>ls -la</bash-input>"},"timestamp":"2026-11-15T10:00:03.000Z"}).to_string();
    let dequeue = json!({"type":"queue-operation","operation":"dequeue","timestamp":"2026-11-15T10:00:02.000Z"}).to_string();
    let got = raw(&[enqueue(1, "ls -la"), dequeue, bash]).unwrap();
    assert!(got.waiting.is_empty());
}

#[test]
fn unknown_queue_operation_is_counted() {
    let shuffle = json!({"type":"queue-operation","operation":"shuffle","timestamp":"2026-11-15T10:00:02.000Z","sessionId":"s"}).to_string();
    let got = raw(&[enqueue(1, OURS), shuffle]).unwrap();
    assert_eq!(got.unknown, 1);
    assert_eq!(got.waiting, [OURS]);
}

#[test]
fn odd_queued_command_prompt_is_counted() {
    let odd = json!({"type":"attachment","attachment":{"type":"queued_command","prompt":7},"timestamp":"2026-11-15T10:00:02.000Z"}).to_string();
    let got = raw(&[enqueue(1, OURS), odd]).unwrap();
    assert_eq!(got.unknown, 1);
    assert_eq!(got.waiting, [OURS]);
}

#[test]
fn missing_file_is_none() {
    assert_eq!(queue_state("/nonexistent/notchling/t.jsonl"), None);
}

#[test]
fn listed_counts_entries_naming_the_message() {
    let got = state("2.1.325", "waiting");
    assert_eq!(got.listed(&message(OURS)), 1);
    assert_eq!(got.listed(&message("  keep   going with\nthe tidy ")), 1);
    assert_eq!(got.listed(&message("something else")), 0);
}

#[test]
fn listed_matches_a_bash_message_and_counts_copies() {
    let got = raw(&[enqueue(1, "ls -la"), enqueue(2, "ok"), enqueue(3, "ok")]).unwrap();
    assert_eq!(got.listed(&message("! ls -la")), 1);
    assert_eq!(got.listed(&message("ok")), 2);
}
