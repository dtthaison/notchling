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

fn stamp(second: u32) -> String {
    format!("2026-11-15T10:00:{second:02}.000Z")
}

fn op(operation: &str, content: Option<&str>, second: u32) -> String {
    let mut line =
        json!({"type":"queue-operation","operation":operation,"timestamp":stamp(second)});
    if let Some(content) = content {
        line["content"] = json!(content);
    }
    line.to_string()
}

fn typed(content: &str, second: u32) -> String {
    json!({"type":"user","timestamp":stamp(second),"message":{"role":"user","content":content}})
        .to_string()
}

fn taken_in_turn(prompt: &str, second: u32) -> String {
    json!({"type":"attachment","timestamp":stamp(second),
        "attachment":{"type":"queued_command","prompt":prompt,"commandMode":"prompt"}})
    .to_string()
}

fn turn_end(second: u32) -> String {
    json!({"type":"system","subtype":"turn_duration","timestamp":stamp(second)}).to_string()
}

fn waiting(lines: &[String]) -> Vec<String> {
    raw(lines).unwrap().waiting
}

#[test]
fn lines_that_are_not_the_prompt_leave_the_dequeue_alone() {
    let result = json!({"type":"user","timestamp":stamp(4),"message":{"role":"user","content":[
        {"type":"tool_result","tool_use_id":"t","content":"ok"}]}})
    .to_string();
    let meta = json!({"type":"user","isMeta":true,"timestamp":stamp(5),
        "message":{"role":"user","content":"Caveat: output below"}})
    .to_string();
    let summary = json!({"type":"user","isCompactSummary":true,"timestamp":stamp(5),
        "message":{"role":"user","content":"This session is being continued"}})
    .to_string();
    let others = [
        typed("[Request interrupted by user for tool use]", 5),
        typed("<local-command-stdout>done</local-command-stdout>", 5),
        typed("<bash-stdout>x</bash-stdout><bash-stderr></bash-stderr>", 5),
        meta,
        summary,
    ];
    for other in others {
        let lines = [
            enqueue(1, "first"),
            enqueue(2, "second"),
            op("dequeue", None, 3),
            result.clone(),
            other.clone(),
            typed("first", 6),
        ];
        assert_eq!(waiting(&lines), ["second"], "{other}");
    }
}

#[test]
fn pop_all_forgets_the_dequeued_entry() {
    let lines = [
        enqueue(1, "a"),
        op("dequeue", None, 2),
        op("popAll", None, 3),
        enqueue(4, "b"),
        enqueue(5, "c"),
        typed("b", 6),
    ];
    assert_eq!(waiting(&lines), ["c"]);
}

#[test]
fn turn_end_forgets_the_dequeued_entry() {
    let lines = [
        enqueue(1, "a"),
        op("dequeue", None, 2),
        turn_end(3),
        enqueue(4, "b"),
        enqueue(5, "c"),
        typed("b", 6),
    ];
    assert_eq!(waiting(&lines), ["c"]);
}

#[test]
fn an_attachment_takes_a_dequeued_entry_out_of_the_batch() {
    let lines = [
        enqueue(1, "a"),
        op("dequeue", None, 2),
        taken_in_turn("a", 3),
        enqueue(4, "b"),
        enqueue(5, "c"),
        typed("b", 6),
    ];
    assert_eq!(waiting(&lines), ["c"]);
}

#[test]
fn a_prompt_that_matches_nothing_clears_the_queue_even_after_a_dequeue() {
    let lines = [
        enqueue(1, "a"),
        enqueue(2, "b"),
        op("dequeue", None, 3),
        typed("something else entirely", 4),
    ];
    assert!(waiting(&lines).is_empty());
}

#[test]
fn an_idle_prompt_drains_the_entries_before_it() {
    let lines = [
        enqueue(1, "a"),
        enqueue(2, "b"),
        enqueue(3, "c"),
        typed("b", 4),
    ];
    assert_eq!(waiting(&lines), vec!["c".to_string()]);
}

#[test]
fn an_entry_taken_in_turn_and_removed_counts_once() {
    let lines = [
        enqueue(1, "yes"),
        enqueue(2, "yes"),
        taken_in_turn("yes", 3),
        op("remove", Some("yes"), 4),
    ];
    assert_eq!(waiting(&lines), ["yes"]);
}

#[test]
fn a_remove_with_no_credit_still_removes_one_copy() {
    let lines = [
        enqueue(1, "yes"),
        enqueue(2, "yes"),
        op("remove", Some("yes"), 3),
    ];
    assert_eq!(waiting(&lines), ["yes"]);
}

#[test]
fn a_credit_does_not_outlive_the_turn() {
    let lines = [
        enqueue(1, "yes"),
        taken_in_turn("yes", 2),
        turn_end(3),
        enqueue(4, "yes"),
        op("remove", Some("yes"), 5),
    ];
    assert!(waiting(&lines).is_empty());
}

#[test]
fn a_notification_delivered_in_a_tool_result_leaves_the_queue() {
    let note = "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>";
    let delivered = json!({"type":"user","timestamp":stamp(5),"message":{"role":"user","content":[
        {"type":"tool_result","tool_use_id":"t","content":"ok"},
        {"type":"text","text":note}]}})
    .to_string();
    let lines = [
        enqueue(1, note),
        enqueue(2, "a"),
        enqueue(3, "b"),
        delivered,
        op("dequeue", None, 6),
        op("dequeue", None, 6),
        typed("a\nb", 7),
    ];
    assert!(waiting(&lines).is_empty());
}

fn picture_only(second: u32) -> String {
    json!({"type":"user","timestamp":stamp(second),"message":{"role":"user","content":[
        {"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}}]}})
    .to_string()
}

#[test]
fn a_picture_only_line_frees_its_entry_like_a_typed_prompt() {
    let lines = [enqueue(1, "[Image #1]"), enqueue(2, "b"), picture_only(3)];
    assert_eq!(waiting(&lines), ["b"]);
}

#[test]
fn a_picture_only_line_after_a_dequeue_consumes_the_batch() {
    let lines = [
        enqueue(1, "[Image #1]"),
        enqueue(2, "b"),
        op("dequeue", None, 3),
        picture_only(4),
    ];
    assert_eq!(waiting(&lines), ["b"]);
}

#[test]
fn an_unrelated_prompt_after_a_two_entry_batch_still_drains_by_match() {
    let lines = [
        enqueue(1, "a"),
        enqueue(2, "b"),
        enqueue(3, "c"),
        enqueue(4, "d"),
        op("dequeue", None, 5),
        op("dequeue", None, 5),
        typed("c", 6),
    ];
    assert_eq!(waiting(&lines), ["d"]);
}

#[test]
fn an_unrelated_prompt_after_a_two_entry_batch_clears_a_lost_queue() {
    let lines = [
        enqueue(1, "a"),
        enqueue(2, "b"),
        enqueue(3, "c"),
        op("dequeue", None, 4),
        op("dequeue", None, 4),
        typed("never queued", 5),
    ];
    assert!(waiting(&lines).is_empty());
}

#[test]
fn a_merged_two_entry_batch_still_frees_both() {
    let lines = [
        enqueue(1, "a"),
        enqueue(2, "b"),
        enqueue(3, "c"),
        op("dequeue", None, 4),
        op("dequeue", None, 4),
        typed("a\nb", 5),
    ];
    assert_eq!(waiting(&lines), ["c"]);
}

#[test]
fn a_remove_matches_its_credit_the_way_entries_are_matched() {
    let lines = [
        enqueue(1, "yes [Image #1]"),
        enqueue(2, "yes"),
        taken_in_turn("yes [Image #1]", 3),
        op("remove", Some("yes"), 4),
    ];
    assert_eq!(waiting(&lines), ["yes"]);
}

#[test]
fn a_drain_forgets_the_credits() {
    let lines = [
        enqueue(1, "yes"),
        enqueue(2, "x"),
        enqueue(3, "yes"),
        taken_in_turn("yes", 4),
        typed("x", 5),
        op("remove", Some("yes"), 6),
    ];
    assert!(waiting(&lines).is_empty());
}

#[test]
fn an_enqueue_of_a_credited_text_drops_the_credit() {
    let lines = [
        enqueue(1, "yes"),
        taken_in_turn("yes", 2),
        enqueue(3, "yes"),
        op("remove", Some("yes"), 4),
    ];
    assert!(waiting(&lines).is_empty());
}

#[test]
fn a_lost_queue_clear_forgets_the_credits() {
    let lines = [
        enqueue(1, "yes"),
        taken_in_turn("yes", 2),
        enqueue(3, "z"),
        typed("never queued", 4),
        enqueue(5, "yes"),
        op("remove", Some("yes"), 6),
    ];
    assert!(waiting(&lines).is_empty());
}
