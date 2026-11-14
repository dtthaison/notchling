use super::*;
use crate::sessions::testutil::TempDir;
use serde_json::json;

fn line(value: serde_json::Value) -> String {
    value.to_string()
}

fn assistant(text: &str) -> String {
    line(
        json!({"type": "assistant", "timestamp": "2026-11-14T22:54:49.559Z",
        "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}}),
    )
}

fn turn_end() -> String {
    line(
        json!({"type": "system", "subtype": "turn_duration", "durationMs": 20000,
        "timestamp": "2026-11-14T22:54:49.593Z"}),
    )
}

fn prompt(text: &str) -> String {
    line(
        json!({"type": "user", "timestamp": "2026-11-14T22:57:14.889Z",
        "message": {"role": "user", "content": text}}),
    )
}

fn user_text(text: &str, meta: bool) -> String {
    line(
        json!({"type": "user", "isMeta": meta, "timestamp": "2026-11-14T22:57:14.889Z",
        "message": {"role": "user", "content": [{"type": "text", "text": text}]}}),
    )
}

fn tool_result() -> String {
    line(
        json!({"type": "user", "timestamp": "2026-11-14T22:57:20.000Z",
        "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}}),
    )
}

fn note(kind: &str) -> String {
    line(json!({"type": kind, "timestamp": "2026-11-14T22:57:15.095Z"}))
}

fn queue(op: &str, content: &str) -> String {
    line(
        json!({"type": "queue-operation", "operation": op, "content": content,
        "timestamp": "2026-11-14T22:57:31.612Z"}),
    )
}

fn away() -> String {
    line(
        json!({"type": "system", "subtype": "away_summary", "content": "summary",
        "timestamp": "2026-11-14T22:58:30.263Z"}),
    )
}

fn tail(lines: &[String]) -> Tail {
    classify(lines.iter().map(String::as_str))
}

#[test]
fn a_turn_that_ended_is_over_whatever_notes_follow_it() {
    let lines = [
        prompt("go"),
        assistant("done"),
        turn_end(),
        note("file-history-snapshot"),
        away(),
    ];
    assert_eq!(tail(&lines), Tail::Over);
}

#[test]
fn a_prompt_typed_after_the_turn_end_means_a_new_turn_though_nothing_else_wrote_yet() {
    let lines = [
        prompt("asks a question"),
        assistant("Do you want it?"),
        turn_end(),
        prompt("yes, both"),
    ];
    assert_eq!(tail(&lines), Tail::Running);
}

#[test]
fn the_requests_and_replies_of_the_new_turn_keep_it_running() {
    let lines = [
        assistant("Do you want it?"),
        turn_end(),
        prompt("yes, both"),
        note("api-request"),
        assistant("on it"),
        queue("enqueue", "d"),
        note("attachment"),
    ];
    assert_eq!(tail(&lines), Tail::Running);
    let lines = [turn_end(), prompt("yes"), assistant("on it"), tool_result()];
    assert_eq!(tail(&lines), Tail::Running, "a tool result is mid turn");
}

#[test]
fn a_finished_background_task_that_wakes_the_session_is_a_new_turn() {
    let lines = [
        turn_end(),
        queue("enqueue", "<task-notification>"),
        queue("dequeue", ""),
        user_text(
            "<task-notification>\n<task-id>b1</task-id></task-notification>",
            true,
        ),
    ];
    assert_eq!(tail(&lines), Tail::Running);
}

#[test]
fn an_interrupted_turn_is_over_though_it_never_wrote_a_turn_end() {
    let lines = [
        turn_end(),
        prompt("run the long thing"),
        assistant("starting"),
        user_text("[Request interrupted by user]", false),
    ];
    assert_eq!(tail(&lines), Tail::Over);
}

#[test]
fn an_api_error_ends_the_turn_too() {
    let error = line(json!({"type": "assistant", "isApiErrorMessage": true,
        "message": {"role": "assistant", "content": [{"type": "text", "text": "API Error"}]}}));
    let lines = [turn_end(), prompt("go"), error];
    assert_eq!(tail(&lines), Tail::Over);
}

#[test]
fn a_slash_command_run_while_idle_is_not_a_turn() {
    let lines = [
        turn_end(),
        user_text(
            "<local-command-caveat>Caveat: the messages below</local-command-caveat>",
            true,
        ),
        user_text("<command-name>/model</command-name>", false),
        user_text(
            "<local-command-stdout>Set model</local-local-command-stdout>",
            false,
        ),
        user_text("<bash-input>ls</bash-input>", false),
    ];
    assert_eq!(tail(&lines), Tail::Over);
}

#[test]
fn a_subagents_lines_do_not_count() {
    let side = line(json!({"type": "assistant", "isSidechain": true,
        "message": {"role": "assistant", "content": [{"type": "text", "text": "x"}]}}));
    let lines = [prompt("go"), assistant("done"), turn_end(), side];
    assert_eq!(tail(&lines), Tail::Over);
}

#[test]
fn a_prompt_with_only_a_picture_starts_a_turn() {
    let picture = line(json!({"type": "user",
        "message": {"role": "user", "content": [{"type": "image", "source": {}}]}}));
    assert_eq!(tail(&[turn_end(), picture]), Tail::Running);
}

#[test]
fn a_file_with_no_conversation_says_nothing() {
    let lines = [
        note("file-history-snapshot"),
        "not json".to_string(),
        away(),
    ];
    assert_eq!(tail(&lines), Tail::Unknown);
    assert_eq!(tail(&[]), Tail::Unknown);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[test]
fn a_transcript_file_is_read_from_its_end() {
    let dir = TempDir::new("restart");
    let ended = dir.write(
        "ended.jsonl",
        &[prompt("go"), assistant("ok"), turn_end()].join("\n"),
    );
    let begun = dir.write(
        "begun.jsonl",
        &[prompt("go"), assistant("ok"), turn_end(), prompt("more")].join("\n"),
    );
    let (ended, begun) = (ended.to_string_lossy(), begun.to_string_lossy());
    assert!(!restarted(&ended, now_ms()));
    assert!(restarted(&begun, now_ms()));
    assert!(!restarted("/nonexistent/none.jsonl", now_ms()));
}

#[test]
fn a_turn_nothing_has_written_to_for_ten_minutes_is_taken_as_over() {
    let dir = TempDir::new("restart-stale");
    let path = dir.write("t.jsonl", &[turn_end(), prompt("more")].join("\n"));
    let path = path.to_string_lossy();
    assert!(restarted(&path, now_ms()));
    assert!(!restarted(&path, now_ms() + STALE_MS + 5_000));
}

#[test]
fn a_last_line_longer_than_the_window_is_a_tool_result_and_means_a_running_turn() {
    let dir = TempDir::new("restart-big");
    let big = line(json!({"type": "user",
        "message": {"role": "user", "content": [{"type": "tool_result", "content": "x".repeat(1_200_000)}]}}));
    let path = dir.write("t.jsonl", &[turn_end(), big].join("\n"));
    assert!(restarted(&path.to_string_lossy(), now_ms()));
}
