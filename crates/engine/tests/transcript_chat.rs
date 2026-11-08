//! Ports of ConsoleCore's ChatTranscriptTests and ChatRichTests: the phone's message view of a
//! transcript. (Pictures, the page cache and message UUIDs are not part of this port.)

mod common;

use common::*;
use notchling_engine::transcript::chat::{self, ChatKind, ChatRole, DiffRowKind};

fn main_jsonl() -> std::path::PathBuf {
    console_fixture("chat/main.jsonl")
}

fn assistant_line(text: &str, uuid: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
    )
}

fn write_lines(scratch: &Scratch, lines: &[String], name: &str) -> String {
    s(&scratch.write(name, &(lines.join("\n") + "\n")))
}

#[test]
fn main_transcript_skips_sidechain_meta_and_injected_wrappers() {
    let result = chat::read(&s(&main_jsonl()), true, chat::DEFAULT_LIMIT);
    let text = result
        .messages
        .iter()
        .map(|m| m.text.as_str())
        .collect::<Vec<_>>()
        .join("|");
    assert!(!text.contains("meta line"));
    assert!(!text.contains("sidechain"));
    assert!(!text.contains("system-reminder"));
    assert!(!text.contains("file body"), "tool results are not chat");
    assert_eq!(
        result.messages.iter().map(|m| m.role).collect::<Vec<_>>(),
        [
            ChatRole::User,
            ChatRole::Tools,
            ChatRole::Assistant,
            ChatRole::User
        ]
    );
    assert_eq!(
        result
            .messages
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>(),
        [
            "Please add a unit test",
            "Read 2 files, ran 1 shell command",
            "Added the test.",
            "Thanks"
        ]
    );
}

#[test]
fn tool_use_blocks_collapse_into_one_summary_message() {
    let result = chat::read(&s(&main_jsonl()), true, chat::DEFAULT_LIMIT);
    let tools: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == ChatRole::Tools)
        .collect();
    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0].tool_summary.as_deref(),
        Some("Read 2 files, ran 1 shell command")
    );
    assert_eq!(
        result
            .messages
            .iter()
            .find(|m| m.role == ChatRole::User)
            .and_then(|m| m.tool_summary.clone()),
        None
    );
}

#[test]
fn malformed_line_is_counted_and_skipped() {
    let result = chat::read(&s(&main_jsonl()), true, chat::DEFAULT_LIMIT);
    assert_eq!(result.malformed_lines, 1);
    assert_eq!(result.messages.len(), 4, "the lines around it still parse");
}

#[test]
fn subagent_transcript_keeps_its_sidechain_lines() {
    let path = s(&console_fixture("chat/subagent.jsonl"));
    let texts: Vec<String> = chat::read(&path, false, 200)
        .messages
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert_eq!(texts, ["Scan the repo", "Scanning now"]);
    assert!(chat::read(&path, true, 200).messages.is_empty());
}

#[test]
fn long_text_is_capped_at_3000_characters() {
    let scratch = Scratch::new("chat");
    let long = write_lines(
        &scratch,
        &[assistant_line(&"x".repeat(3500), "long")],
        "long.jsonl",
    );
    let message = chat::read(&long, true, 200).messages.remove(0);
    assert_eq!(message.text.chars().count(), 3000);
    assert!(message.text.ends_with('…'));
    let exact = write_lines(
        &scratch,
        &[assistant_line(&"y".repeat(3000), "exact")],
        "exact.jsonl",
    );
    assert_eq!(
        chat::read(&exact, true, 200).messages[0]
            .text
            .chars()
            .count(),
        3000
    );
}

#[test]
fn lines_over_100kb_are_skipped() {
    let scratch = Scratch::new("chat");
    let path = write_lines(
        &scratch,
        &[
            assistant_line("before", "u1"),
            assistant_line(&"z".repeat(150_000), "huge"),
            r#"{"type":"user","uuid":"u3","message":{"role":"user","content":"after"}}"#
                .to_string(),
        ],
        "huge.jsonl",
    );
    let texts: Vec<String> = chat::read(&path, true, 200)
        .messages
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert_eq!(texts, ["before", "after"]);
}

#[test]
fn a_line_is_judged_by_its_size_without_the_base64_pictures() {
    // A screenshot makes a line big only because of its base64; ChatTranscript cuts that out first, so
    // the message with the picture is still shown.
    let scratch = Scratch::new("chat-image");
    let picture = "A".repeat(150_000);
    let with_picture = format!(
        r#"{{"type":"user","uuid":"p1","message":{{"role":"user","content":[{{"type":"text","text":"look at this"}},{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{picture}"}}}}]}}}}"#
    );
    assert!(with_picture.len() > 100_000);
    let path = write_lines(
        &scratch,
        &[
            assistant_line("before", "u1"),
            with_picture,
            assistant_line("after", "u3"),
        ],
        "image.jsonl",
    );
    let result = chat::read(&path, true, 200);
    let texts: Vec<&str> = result.messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["before", "look at this", "after"]);
    assert_eq!(result.malformed_lines, 0);
}

#[test]
fn a_line_still_big_after_the_pictures_are_cut_is_skipped() {
    let scratch = Scratch::new("chat-image-big");
    let big = format!(
        r#"{{"type":"user","uuid":"p1","message":{{"role":"user","content":[{{"type":"text","text":"{}"}},{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"QUJD"}}}}]}}}}"#,
        "z".repeat(150_000)
    );
    let path = write_lines(&scratch, &[assistant_line("kept", "u1"), big], "big.jsonl");
    let texts: Vec<String> = chat::read(&path, true, 200)
        .messages
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert_eq!(texts, ["kept"]);
}

#[test]
fn a_window_that_starts_inside_a_long_line_reads_that_line_whole() {
    // Swift backs up to where the line starts instead of dropping it (a long line: a screenshot, say).
    let scratch = Scratch::new("chat-window");
    let long = assistant_line(&"x".repeat(5_000), "long");
    let path = write_lines(
        &scratch,
        &[
            assistant_line("first", "u1"),
            long.clone(),
            assistant_line("last", "u3"),
        ],
        "window.jsonl",
    );
    // Starts 1500 bytes before the end: inside the long line.
    let lines = chat::read_window(&path, 1_500).expect("window");
    assert_eq!(lines.len(), 2, "the long line and the last one");
    assert_eq!(lines[0].text, long);
    assert_eq!(
        lines[0].offset,
        (assistant_line("first", "u1").len() + 1) as u64
    );
    assert!(lines[1].text.contains("last"));
}

#[test]
fn a_window_that_starts_on_a_line_start_drops_nothing() {
    let scratch = Scratch::new("chat-window-aligned");
    let second = assistant_line("second", "u2");
    let path = write_lines(
        &scratch,
        &[assistant_line("first", "u1"), second.clone()],
        "aligned.jsonl",
    );
    let lines = chat::read_window(&path, second.len() as u64 + 1).expect("window");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, second);
}

#[test]
fn returns_only_the_last_200_messages() {
    let scratch = Scratch::new("chat");
    let lines: Vec<String> = (0..250)
        .map(|i| {
            let role = if i % 2 == 0 { "user" } else { "assistant" };
            format!(r#"{{"type":"{role}","uuid":"n{i}","message":{{"role":"{role}","content":"message {i}"}}}}"#)
        })
        .collect();
    let path = write_lines(&scratch, &lines, "many.jsonl");
    let messages = chat::read(&path, true, chat::DEFAULT_LIMIT).messages;
    assert_eq!(messages.len(), 200);
    assert_eq!(messages[0].text, "message 50");
    assert_eq!(messages[199].text, "message 249");
}

#[test]
fn only_the_tail_is_read_and_the_cut_line_is_dropped() {
    let scratch = Scratch::new("chat");
    let lines: Vec<String> = (0..50)
        .map(|i| format!(r#"{{"type":"user","uuid":"t{i}","message":{{"role":"user","content":"line {i}"}}}}"#))
        .collect();
    let path = write_lines(&scratch, &lines, "tail.jsonl");
    let tail = chat::tail(&path, 400).expect("tail");
    assert!(tail.len() < 50);
    let result = chat::parse(&tail, true, &path, chat::DEFAULT_LIMIT);
    assert_eq!(
        result.malformed_lines, 0,
        "a half line at the cut is not reported as malformed"
    );
    // Adjacent user lines merge into one message.
    assert!(result
        .messages
        .last()
        .is_some_and(|m| m.text.ends_with("line 49")));
    assert!(!result.messages.iter().any(|m| m.text.starts_with("line 0")));
}

#[test]
fn message_ids_are_stable_across_reads() {
    let first: Vec<String> = chat::read(&s(&main_jsonl()), true, 200)
        .messages
        .into_iter()
        .map(|m| m.id)
        .collect();
    let second: Vec<String> = chat::read(&s(&main_jsonl()), true, 200)
        .messages
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(first, second);
    let unique: std::collections::HashSet<&String> = first.iter().collect();
    assert_eq!(unique.len(), first.len());
}

// MARK: rich messages

fn assistant(id: &str, blocks: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{id}","timestamp":"2026-11-04T08:00:01.000Z","message":{{"role":"assistant","model":"m","content":[{blocks}]}}}}"#
    )
}

fn user(id: &str, content: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{id}","timestamp":"2026-11-04T08:00:02.000Z","message":{{"role":"user","content":{content}}}}}"#
    )
}

fn parse(lines: &[String]) -> Vec<chat::ChatMessage> {
    chat::parse(lines, true, "x", chat::DEFAULT_LIMIT).messages
}

#[test]
fn an_edit_carries_its_diff_and_a_bash_call_its_output() {
    let messages = parse(&[
        assistant(
            "a1",
            r#"{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/p/lib/auth/guard.dart","old_string":"if (a) return 1;","new_string":"await ready;\nif (a) return 1;"}},{"type":"tool_use","id":"t2","name":"Bash","input":{"command":"flutter test test/auth\nsecond line"}}"#,
        ),
        user(
            "u1",
            r#"[{"type":"tool_result","tool_use_id":"t2","content":"00:03 +14: All tests passed!"}]"#,
        ),
    ]);
    assert_eq!(messages.len(), 1);
    let steps = &messages[0].steps;
    assert_eq!(
        steps.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["Edit", "Bash"]
    );
    assert_eq!(steps[0].summary, "auth/guard.dart");
    assert_eq!(steps[0].path.as_deref(), Some("/p/lib/auth/guard.dart"));
    assert_eq!(
        steps[0].diff.iter().map(|d| d.wire()).collect::<Vec<_>>(),
        ["-if (a) return 1;", "+await ready;", "+if (a) return 1;"]
    );
    assert_eq!(
        steps[1].summary, "flutter test test/auth",
        "only the first line of a command"
    );
    assert_eq!(
        steps[1].output.as_deref(),
        Some("00:03 +14: All tests passed!")
    );
}

#[test]
fn a_result_that_arrives_after_the_run_was_folded_still_lands_on_its_step() {
    let messages = parse(&[
        assistant(
            "a1",
            r#"{"type":"text","text":"Reading."},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/p/a.swift"}}"#,
        ),
        user(
            "u1",
            r#"[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"import Foundation"}]}]"#,
        ),
    ]);
    let tools = messages
        .iter()
        .find(|m| m.role == ChatRole::Tools)
        .expect("tools");
    assert_eq!(tools.steps[0].output.as_deref(), Some("import Foundation"));
}

#[test]
fn a_write_shows_its_first_lines_as_added() {
    let messages = parse(&[assistant(
        "a1",
        r#"{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/p/n.md","content":"one\ntwo"}}"#,
    )]);
    assert_eq!(
        messages[0].steps[0]
            .diff
            .iter()
            .map(|d| d.wire())
            .collect::<Vec<_>>(),
        ["+one", "+two"]
    );
}

#[test]
fn a_shell_command_and_its_output_become_one_message() {
    let messages = parse(&[
        user("u1", r#""<bash-input>git status --short</bash-input>""#),
        user(
            "u2",
            r#""<bash-stdout> M a.dart\n?? b.md</bash-stdout><bash-stderr></bash-stderr>""#,
        ),
    ]);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].kind, ChatKind::Shell);
    assert_eq!(messages[0].role, ChatRole::User);
    assert_eq!(messages[0].text, "!git status --short");
    assert_eq!(messages[0].output.as_deref(), Some("M a.dart\n?? b.md"));
}

#[test]
fn compaction_is_one_notice_and_its_summary_is_not_a_user_message() {
    let messages = parse(&[
        assistant("a1", r#"{"type":"text","text":"before"}"#),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"c1","timestamp":"2026-11-04T08:00:03.000Z","compactMetadata":{"trigger":"auto","preTokens":168000}}"#.to_string(),
        r#"{"type":"user","uuid":"u9","isCompactSummary":true,"timestamp":"2026-11-04T08:00:04.000Z","message":{"role":"user","content":"This session is being continued from a previous conversation..."}}"#.to_string(),
        assistant("a2", r#"{"type":"text","text":"after"}"#),
    ]);
    assert_eq!(
        messages.iter().map(|m| m.kind).collect::<Vec<_>>(),
        [ChatKind::Text, ChatKind::Compaction, ChatKind::Text]
    );
    assert_eq!(messages[1].text, "168k tokens");
    assert!(!messages.iter().any(|m| m.text.contains("being continued")));
}

#[test]
fn long_output_is_cut() {
    let big = "x".repeat(5000);
    let messages = parse(&[
        assistant(
            "a1",
            r#"{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cat big"}}"#,
        ),
        user(
            "u1",
            &format!(r#"[{{"type":"tool_result","tool_use_id":"t1","content":"{big}"}}]"#),
        ),
    ]);
    assert!(
        messages[0].steps[0]
            .output
            .as_deref()
            .map_or(0, |o| o.chars().count())
            <= 3000
    );
}

#[test]
fn diff_rows_use_the_wire_prefixes() {
    assert_eq!(
        chat::DiffRow {
            kind: DiffRowKind::Removed,
            text: "x".into()
        }
        .wire(),
        "-x"
    );
    assert_eq!(
        chat::DiffRow {
            kind: DiffRowKind::Context,
            text: "x".into()
        }
        .wire(),
        " x"
    );
}

#[test]
fn a_message_taken_from_the_queue_is_shown_once_and_a_waiting_one_is_marked() {
    let messages = parse(&[
        user("u0", r#""first""#),
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-04T08:00:03.000Z","content":"later"}"#.to_string(),
    ]);
    assert_eq!(messages.len(), 2);
    assert!(!messages[0].queued);
    assert!(messages[1].queued);
    assert_eq!(messages[1].text, "later");
}
