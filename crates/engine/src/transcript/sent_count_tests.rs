use super::*;
use crate::sessions::iso::parse_iso_ms;
use crate::sessions::testutil::TempDir;
use crate::transcript::message::Message;
use serde_json::json;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/outbox")
        .join(name);
    path.to_string_lossy().into_owned()
}

fn at(seconds: u32) -> i64 {
    parse_iso_ms(&format!(
        "2026-11-15T10:{:02}:{:02}.000Z",
        seconds / 60,
        seconds % 60
    ))
    .unwrap()
}

fn message(text: &str) -> Message {
    Message::parse(text, |_| false)
}

fn count(name: &str, text: &str, since: u32) -> Option<Seen> {
    count_since(&fixture(name), &message(text), at(since))
}

fn seen(enqueued: usize, taken: usize) -> Option<Seen> {
    Some(Seen { enqueued, taken })
}

fn count_lines(lines: &[String], text: &str, since: u32) -> Option<Seen> {
    let dir = TempDir::new("count-since");
    let path = dir.write("t.jsonl", &lines.join("\n"));
    count_since(&path.to_string_lossy(), &message(text), at(since))
}

#[test]
fn counts_each_copy_after_since() {
    assert_eq!(count("counts.jsonl", "ok", 0), seen(0, 2));
    assert_eq!(count("counts.jsonl", "ok", 20), seen(0, 1));
    assert_eq!(count("counts.jsonl", "ok", 40), seen(0, 0));
}

#[test]
fn enqueue_counts_as_enqueued_not_taken() {
    assert_eq!(count("queue_ops.jsonl", "lonely", 0), seen(1, 0));
}

#[test]
fn queued_command_and_user_line_count_as_taken() {
    assert_eq!(count("queue_ops.jsonl", "three", 0), seen(1, 1));
    assert_eq!(count("queue_ops.jsonl", "ok", 15), seen(1, 1));
}

#[test]
fn dequeue_counts_only_through_its_user_line() {
    assert_eq!(count("queue_ops.jsonl", "lonely", 0), seen(1, 0));
    assert_eq!(count("queue_ops.jsonl", "ok", 35), seen(0, 0));
}

#[test]
fn merged_user_line_counts_as_taken_for_each_entry() {
    assert_eq!(count("merged.jsonl", "hi", 0), seen(2, 2));
    assert_eq!(count("merged.jsonl", "a", 0), seen(1, 1));
    assert_eq!(count("merged.jsonl", "b", 0), seen(1, 1));
    assert_eq!(count("merged.jsonl", "a b", 0), seen(0, 0));
    assert_eq!(count("merged.jsonl", "hi hi", 0), seen(0, 0));
}

#[test]
fn merged_line_counts_only_entries_after_since() {
    assert_eq!(count("merged.jsonl", "hi", 11), seen(0, 2));
    assert_eq!(count("merged.jsonl", "hi", 13), seen(0, 0));
}

#[test]
fn pop_all_and_remove_count_in_neither() {
    assert_eq!(count("queue_ops.jsonl", "go on", 0), seen(1, 0));
    assert_eq!(count("queue_ops.jsonl", "two", 0), seen(1, 0));
}

#[test]
fn image_message_matches_chip_line() {
    let with_picture = Message {
        text: "why?".to_string(),
        pictures: 1,
        bash: None,
        command: None,
        paths: Vec::new(),
    };
    assert_eq!(
        count_since(&fixture("images.jsonl"), &with_picture, at(0)),
        seen(0, 1)
    );
    let two = Message {
        pictures: 2,
        ..with_picture.clone()
    };
    assert_eq!(
        count_since(&fixture("images.jsonl"), &two, at(0)),
        seen(0, 0)
    );
    assert_eq!(count("images.jsonl", "why?", 0), seen(0, 2));
}

#[test]
fn bash_matches_enqueue_and_bash_input() {
    assert_eq!(count("bash.jsonl", "!git branch", 0), seen(1, 1));
    assert_eq!(count("bash.jsonl", "git status", 0), seen(0, 0));
}

#[test]
fn command_matches_local_command_line() {
    assert_eq!(count("command.jsonl", "/model opus", 0), seen(0, 1));
    assert_eq!(count("command.jsonl", "/model sonnet", 0), seen(0, 0));
}

#[test]
fn older_command_name_user_line() {
    assert_eq!(count("command.jsonl", "/effort high", 0), seen(0, 1));
    assert_eq!(count("command.jsonl", "/effort low", 0), seen(0, 0));
}

#[test]
fn long_paste_matches_full_text() {
    let text = format!("{}\n\n{}", "a long line of words ".repeat(800), "end");
    let lines = [
        json!({"type": "user", "timestamp": "2026-11-15T10:00:10.000Z",
        "message": {"role": "user", "content": text.replace(' ', "  ")}})
        .to_string(),
    ];
    assert!(text.len() > 16_000);
    assert_eq!(count_lines(&lines, &text, 0), seen(0, 1));
}

#[test]
fn none_when_window_starts_after_since() {
    let filler = "x".repeat(900);
    let lines: Vec<String> = (0..1_400)
        .map(|_| {
            json!({"type": "user", "timestamp": "2026-11-15T10:59:00.000Z",
                "message": {"role": "user", "content": filler}})
            .to_string()
        })
        .collect();
    assert_eq!(count_lines(&lines, "ok", 0), None);
    assert_eq!(count_lines(&lines, "ok", 59 * 60 + 30), seen(0, 0));
}

#[test]
fn none_for_a_missing_file() {
    assert_eq!(
        count_since("/nonexistent/none.jsonl", &message("ok"), at(0)),
        None
    );
}

#[test]
fn subagent_and_tool_result_lines_never_count() {
    let lines = [
        json!({"type": "user", "isSidechain": true, "timestamp": "2026-11-15T10:00:10.000Z",
            "message": {"role": "user", "content": "ok"}})
        .to_string(),
        json!({"type": "user", "timestamp": "2026-11-15T10:00:20.000Z",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t", "content": "ok"},
                {"type": "text", "text": "ok"}]}})
        .to_string(),
        json!({"type": "user", "isMeta": true, "timestamp": "2026-11-15T10:00:30.000Z",
            "message": {"role": "user", "content": "ok"}})
        .to_string(),
        json!({"type": "assistant", "timestamp": "2026-11-15T10:00:40.000Z",
            "message": {"role": "assistant", "content": [{"type": "text", "text": "ok"}]}})
        .to_string(),
    ];
    assert_eq!(count_lines(&lines, "ok", 0), seen(0, 0));
}

#[test]
fn an_empty_message_matches_nothing() {
    assert_eq!(count("counts.jsonl", "", 0), seen(0, 0));
}

fn stamp(second: u32) -> String {
    format!("2026-11-15T10:00:{second:02}.000Z")
}

fn op(operation: &str, content: Option<&str>, second: u32) -> String {
    let mut line = json!({"type": "queue-operation", "operation": operation,
        "timestamp": stamp(second)});
    if let Some(content) = content {
        line["content"] = json!(content);
    }
    line.to_string()
}

fn user(content: serde_json::Value, second: u32) -> String {
    json!({"type": "user", "timestamp": stamp(second),
        "message": {"role": "user", "content": content}})
    .to_string()
}

#[test]
fn meta_sidechain_and_summary_lines_never_count() {
    let command = "<command-name>/model</command-name><command-args>opus</command-args>";
    for key in ["isMeta", "isSidechain", "isCompactSummary"] {
        let mut meta_user = json!({"type": "user", "timestamp": stamp(10),
            "message": {"role": "user", "content": command}});
        meta_user[key] = json!(true);
        let mut meta_system = json!({"type": "system", "subtype": "local_command",
            "timestamp": stamp(20), "content": command});
        meta_system[key] = json!(true);
        let mut meta_attachment = json!({"type": "attachment", "timestamp": stamp(30),
            "attachment": {"type": "queued_command", "prompt": "/model opus"}});
        meta_attachment[key] = json!(true);
        let lines = [
            meta_user.to_string(),
            meta_system.to_string(),
            meta_attachment.to_string(),
        ];
        assert_eq!(count_lines(&lines, "/model opus", 0), seen(0, 0), "{key}");
    }
}

#[test]
fn picture_only_message_is_taken_by_an_image_only_line() {
    let picture = Message {
        text: String::new(),
        pictures: 1,
        bash: None,
        command: None,
        paths: Vec::new(),
    };
    let image = json!({"type": "image", "source": {"type": "base64", "data": "x"}});
    let lines = [
        user(json!([image.clone()]), 10),
        user(json!("[Image #3]"), 20),
        user(
            json!([image.clone(), {"type": "text", "text": "[Image #4]"}]),
            30,
        ),
        json!({"type": "attachment", "timestamp": stamp(40),
            "attachment": {"type": "queued_command", "prompt": [image.clone()]}})
        .to_string(),
        user(json!([{"type": "text", "text": "plain"}]), 50),
        user(
            json!([{"type": "tool_result", "tool_use_id": "t", "content": "x"}, image]),
            55,
        ),
    ];
    let dir = TempDir::new("count-since");
    let path = dir.write("t.jsonl", &lines.join("\n"));
    assert_eq!(
        count_since(&path.to_string_lossy(), &picture, at(0)),
        seen(0, 3)
    );
}

#[test]
fn merged_line_needs_the_pictures_too() {
    let with_picture = Message {
        text: "hi".to_string(),
        pictures: 1,
        bash: None,
        command: None,
        paths: Vec::new(),
    };
    assert_eq!(
        count_since(&fixture("merged.jsonl"), &with_picture, at(0)),
        seen(0, 0)
    );
}

#[test]
fn a_dequeued_notification_does_not_break_a_later_merge() {
    let lines = [
        op(
            "enqueue",
            Some("<task-notification>done</task-notification>"),
            1,
        ),
        op("dequeue", None, 2),
        user(json!("<task-notification>done</task-notification>"), 3),
        op("enqueue", Some("a"), 4),
        op("enqueue", Some("b"), 5),
        op("dequeue", None, 6),
        op("dequeue", None, 6),
        user(json!("a\nb"), 7),
    ];
    assert_eq!(count_lines(&lines, "a", 0), seen(1, 1));
    assert_eq!(count_lines(&lines, "b", 0), seen(1, 1));
}

#[test]
fn pop_all_clears_a_dequeued_entry_before_a_later_merge() {
    let lines = [
        op("enqueue", Some("x"), 1),
        op("dequeue", None, 2),
        op("popAll", None, 3),
        op("enqueue", Some("a"), 4),
        op("enqueue", Some("b"), 5),
        op("dequeue", None, 6),
        op("dequeue", None, 6),
        user(json!("a\nb"), 7),
    ];
    assert_eq!(count_lines(&lines, "a", 0), seen(1, 1));
    assert_eq!(count_lines(&lines, "b", 0), seen(1, 1));
}

#[test]
fn an_interrupt_line_between_dequeue_and_merged_prompt_keeps_the_batch() {
    let lines = [
        op("enqueue", Some("a"), 1),
        op("enqueue", Some("b"), 2),
        op("dequeue", None, 3),
        op("dequeue", None, 3),
        user(json!("[Request interrupted by user for tool use]"), 4),
        user(json!("a\nb"), 5),
    ];
    assert_eq!(count_lines(&lines, "a", 0), seen(1, 1));
    assert_eq!(count_lines(&lines, "b", 0), seen(1, 1));
}

#[test]
fn a_notification_delivered_in_a_tool_result_does_not_break_a_later_merge() {
    let note = "<task-notification>done</task-notification>";
    let delivered = user(
        json!([{"type": "tool_result", "tool_use_id": "t", "content": "ok"},
            {"type": "text", "text": note}]),
        4,
    );
    let lines = [
        op("enqueue", Some(note), 1),
        op("enqueue", Some("a"), 2),
        op("enqueue", Some("b"), 3),
        delivered,
        op("dequeue", None, 5),
        op("dequeue", None, 5),
        user(json!("a\nb"), 6),
    ];
    assert_eq!(count_lines(&lines, "a", 0), seen(1, 1));
}

#[test]
fn a_turn_end_forgets_a_stale_dequeue_for_a_later_merge() {
    let lines = [
        op("enqueue", Some("x"), 1),
        op("dequeue", None, 2),
        json!({"type": "system", "subtype": "turn_duration", "timestamp": stamp(3)}).to_string(),
        op("enqueue", Some("a"), 4),
        op("enqueue", Some("b"), 5),
        op("dequeue", None, 6),
        op("dequeue", None, 6),
        user(json!("a\nb"), 7),
    ];
    assert_eq!(count_lines(&lines, "a", 0), seen(1, 1));
}
