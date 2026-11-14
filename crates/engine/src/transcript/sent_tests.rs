use super::*;
use crate::sessions::testutil::TempDir;
use serde_json::json;

const LONG: &str = "Please add the retry to the upload step and then run the whole suite twice to be sure it holds";

fn user_at(stamp: &str, text: &str) -> String {
    json!({"type": "user", "timestamp": stamp,
        "message": {"role": "user", "content": text}})
    .to_string()
}

fn blocks_at(stamp: &str, text: &str) -> String {
    json!({"type": "user", "timestamp": stamp,
        "message": {"role": "user", "content": [{"type": "text", "text": text}]}})
    .to_string()
}

fn enqueue_at(stamp: &str, text: &str) -> String {
    json!({"type": "queue-operation", "operation": "enqueue", "content": text, "timestamp": stamp})
        .to_string()
}

fn assistant_at(stamp: &str, text: &str) -> String {
    json!({"type": "assistant", "timestamp": stamp,
        "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}})
    .to_string()
}

fn since() -> i64 {
    crate::sessions::iso::parse_iso_ms("2026-11-14T22:57:40.000Z").unwrap()
}

fn check(lines: &[String], text: &str) -> Option<bool> {
    let dir = TempDir::new("sent");
    let path = dir.write("t.jsonl", &lines.join("\n"));
    sent_since(&path.to_string_lossy(), text, since())
}

#[test]
fn a_user_line_after_the_attempt_that_holds_the_text_means_it_was_sent() {
    let lines = [
        user_at("2026-11-14T22:57:14.000Z", "yes, both"),
        user_at("2026-11-14T22:57:50.000Z", LONG),
    ];
    assert_eq!(check(&lines, LONG), Some(true));
}

#[test]
fn the_text_inside_a_longer_message_a_person_sent_counts_too() {
    let mixed = format!("also this {LONG} thanks");
    let lines = [
        user_at("2026-11-14T22:57:14.000Z", "yes, both"),
        blocks_at("2026-11-14T22:57:50.000Z", &mixed),
    ];
    assert_eq!(check(&lines, LONG), Some(true));
}

#[test]
fn line_breaks_and_runs_of_spaces_do_not_hide_it() {
    let wrapped = LONG.replace(' ', "  \n");
    let lines = [
        user_at("2026-11-14T22:57:14.000Z", "go"),
        user_at("2026-11-14T22:57:50.000Z", &wrapped),
    ];
    assert_eq!(check(&lines, LONG), Some(true));
}

#[test]
fn a_cut_down_copy_with_the_start_or_the_end_of_it_counts_as_maybe_sent() {
    let start = &LONG[..50];
    let end = &LONG[LONG.len() - 50..];
    for part in [start, end] {
        let lines = [
            user_at("2026-11-14T22:57:14.000Z", "go"),
            user_at("2026-11-14T22:57:50.000Z", part),
        ];
        assert_eq!(check(&lines, LONG), Some(true), "{part}");
    }
}

#[test]
fn a_message_queued_while_the_session_worked_counts() {
    let lines = [
        user_at("2026-11-14T22:57:14.000Z", "go"),
        enqueue_at("2026-11-14T22:57:50.000Z", LONG),
    ];
    assert_eq!(check(&lines, LONG), Some(true));
}

#[test]
fn the_same_text_from_before_the_attempt_does_not_count() {
    let lines = [
        user_at("2026-11-14T22:57:14.000Z", LONG),
        assistant_at("2026-11-14T22:57:30.000Z", "done"),
        user_at("2026-11-14T22:57:50.000Z", "something else"),
    ];
    assert_eq!(check(&lines, LONG), Some(false));
}

#[test]
fn an_assistant_line_or_a_tool_result_that_repeats_it_is_not_a_message_from_a_person() {
    let echo = json!({"type": "user", "timestamp": "2026-11-14T22:57:50.000Z",
        "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t", "content": LONG}]}})
    .to_string();
    let lines = [
        user_at("2026-11-14T22:57:14.000Z", "go"),
        assistant_at("2026-11-14T22:57:45.000Z", LONG),
        echo,
    ];
    assert_eq!(check(&lines, LONG), Some(false));
}

#[test]
fn a_file_that_is_missing_cannot_say() {
    assert_eq!(
        sent_since("/nonexistent/none.jsonl", LONG, since()),
        None,
        "unsure is not no"
    );
}

#[test]
fn a_window_that_starts_after_the_attempt_cannot_say() {
    let dir = TempDir::new("sent-window");
    let filler = "x".repeat(2_000);
    let mut lines = Vec::new();
    for n in 0..800 {
        lines.push(user_at(
            &format!("2026-11-14T23:{:02}:{:02}.000Z", n / 60 % 60, n % 60),
            &filler,
        ));
    }
    let path = dir.write("t.jsonl", &lines.join("\n"));
    assert!(std::fs::metadata(&path).unwrap().len() > WINDOW_BYTES);
    assert_eq!(sent_since(&path.to_string_lossy(), LONG, since()), None);
}

#[test]
fn an_empty_text_was_never_sent() {
    assert_eq!(
        check(&[user_at("2026-11-14T22:57:50.000Z", "x")], "  "),
        Some(false)
    );
}
