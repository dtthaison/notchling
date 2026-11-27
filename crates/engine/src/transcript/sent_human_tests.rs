use super::*;
use crate::sessions::iso::parse_iso_ms;
use crate::sessions::testutil::TempDir;
use crate::transcript::message::Message;
use crate::transcript::queue_state::queue_lists;
use serde_json::{json, Value};

fn stamp(second: u32) -> String {
    format!("2026-11-17T16:16:{second:02}.000Z")
}

fn since(second: u32) -> i64 {
    parse_iso_ms(&stamp(second)).unwrap()
}

fn message(text: &str) -> Message {
    Message::parse(text, |_| false)
}

fn seen(enqueued: usize, taken: usize) -> Option<Seen> {
    Some(Seen { enqueued, taken })
}

fn write(lines: &[String]) -> (TempDir, String) {
    let dir = TempDir::new("sent-human");
    let path = dir.write("t.jsonl", &format!("{}\n", lines.join("\n")));
    let path = path.to_string_lossy().into_owned();
    (dir, path)
}

fn enqueue(content: &str, second: u32) -> String {
    json!({"type": "queue-operation", "operation": "enqueue",
        "timestamp": stamp(second), "sessionId": "s", "content": content})
    .to_string()
}

fn user(text: &str, origin: Option<&str>, second: u32) -> String {
    let mut line = json!({"type": "user", "timestamp": stamp(second),
        "message": {"role": "user", "content": text}});
    if let Some(kind) = origin {
        line["origin"] = json!({"kind": kind});
    }
    line.to_string()
}

fn attachment(text: &str, origin: &str, second: u32) -> String {
    json!({"type": "attachment", "timestamp": stamp(second),
        "attachment": {"type": "queued_command", "prompt": text, "origin": {"kind": origin}}})
    .to_string()
}

fn lead() -> String {
    json!({"type": "assistant", "timestamp": stamp(0),
        "message": {"role": "assistant", "content": [{"type": "text", "text": "earlier"}]}})
    .to_string()
}

const MARKUP: &str = "<!-- snippet start -->\n<div class=\"a\">one</div>\n<!-- snippet end -->";

#[test]
fn a_human_line_that_starts_with_a_tag_is_a_take() {
    let lines = [lead(), user(MARKUP, Some("human"), 20)];
    let (_dir, path) = write(&lines);
    assert_eq!(count_since(&path, &message(MARKUP), since(18)), seen(0, 1));
}

#[test]
fn a_human_attachment_that_starts_with_a_tag_is_a_take() {
    let lines = [lead(), enqueue(MARKUP, 20), attachment(MARKUP, "human", 20)];
    let (_dir, path) = write(&lines);
    assert_eq!(count_since(&path, &message(MARKUP), since(18)), seen(1, 1));
}

#[test]
fn a_tagged_line_from_another_origin_is_not_a_take() {
    for origin in [None, Some("task-notification"), Some("agent")] {
        let lines = [lead(), user(MARKUP, origin, 20)];
        let (_dir, path) = write(&lines);
        assert_eq!(
            count_since(&path, &message(MARKUP), since(18)),
            seen(0, 0),
            "{origin:?}"
        );
    }
    let lines = [lead(), attachment(MARKUP, "task-notification", 20)];
    let (_dir, path) = write(&lines);
    assert_eq!(count_since(&path, &message(MARKUP), since(18)), seen(0, 0));
}

#[test]
fn a_long_paste_wrapped_by_claude_code_matches_its_body() {
    let body = "first pasted row\nsecond pasted row\n\nthird pasted row";
    let wrapped =
        format!("\n\n<pasted_content id=\"a1b2\">\n{body}\n</pasted_content id=\"a1b2\">\n");
    let lines = [lead(), user(&wrapped, Some("human"), 20)];
    let (_dir, path) = write(&lines);
    assert_eq!(count_since(&path, &message(body), since(18)), seen(0, 1));
    assert_eq!(
        count_since(&path, &message("first pasted row"), since(18)),
        seen(0, 0)
    );
}

#[test]
fn a_paste_with_typed_words_around_it_matches() {
    let wrapped = "look at this\n\n<pasted_content id=\"q9\">\nrow a\nrow b\n</pasted_content id=\"q9\">\n\nthanks";
    let lines = [lead(), attachment(wrapped, "human", 20)];
    let (_dir, path) = write(&lines);
    let text = "look at this\n\nrow a\nrow b\n\nthanks";
    assert_eq!(count_since(&path, &message(text), since(18)), seen(0, 1));
}

#[test]
fn a_command_line_is_still_taken_through_its_command_name() {
    let command = "<command-name>/model</command-name><command-args>opus</command-args>";
    let lines = [lead(), user(command, Some("human"), 20)];
    let (_dir, path) = write(&lines);
    assert_eq!(
        count_since(&path, &message("/model opus"), since(18)),
        seen(0, 1)
    );
}

#[test]
fn a_waiting_tagged_entry_is_listed_for_its_own_message() {
    let lines = [lead(), enqueue(MARKUP, 20)];
    let (_dir, path) = write(&lines);
    assert_eq!(queue_lists(&path, &message(MARKUP)), Some(true));
    assert_eq!(queue_lists(&path, &message("another")), Some(false));
    let state = crate::transcript::queue_state::queue_state(&path).unwrap();
    assert_eq!(
        state.waiting,
        Vec::<String>::new(),
        "the gate still ignores entries that are not prompts"
    );
}

#[test]
fn a_waiting_entry_leaves_the_list_on_a_human_tagged_take() {
    let lines = [lead(), enqueue(MARKUP, 20), attachment(MARKUP, "human", 21)];
    let (_dir, path) = write(&lines);
    assert_eq!(queue_lists(&path, &message(MARKUP)), Some(false));
}

#[test]
fn json_value_origin_helper_is_not_confused_by_other_kinds() {
    let line: Value = serde_json::from_str(&user("plain words", Some("human"), 20)).unwrap();
    assert!(took_prompt(&line, "plain words", 0));
    let line: Value = serde_json::from_str(&user("<x>", Some("task-notification"), 20)).unwrap();
    assert!(!took_prompt(&line, "<x>", 0));
}
