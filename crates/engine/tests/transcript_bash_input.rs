mod common;

use common::*;
use notchling_engine::transcript::chat::{self, ChatKind, ChatMessage};
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::model::*;
use std::path::Path;

const ENQUEUE_BASH: &str = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-13T09:11:52.665Z","content":"git branch"}"#;
const DEQUEUE: &str =
    r#"{"type":"queue-operation","operation":"dequeue","timestamp":"2026-11-13T09:11:52.797Z"}"#;
const TURN_END: &str = r#"{"type":"system","subtype":"turn_duration","durationMs":42000,"timestamp":"2026-11-13T09:11:52.800Z"}"#;
const BASH_INPUT: &str = r#"{"type":"user","message":{"role":"user","content":"<bash-input>git branch</bash-input>"},"timestamp":"2026-11-13T09:11:52.803Z"}"#;
const BASH_OUTPUT: &str = r#"{"type":"user","message":{"role":"user","content":"<bash-stdout>* main</bash-stdout><bash-stderr></bash-stderr>"},"timestamp":"2026-11-13T09:11:53.100Z"}"#;
const ENQUEUE_NEXT: &str = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-13T09:12:10.000Z","content":"and then fix the build"}"#;

fn fixture_lines() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/queue/bash-input.jsonl");
    std::fs::read_to_string(path)
        .expect("fixture")
        .lines()
        .map(String::from)
        .collect()
}

fn read(lines: &[String]) -> Conversation {
    let scratch = Scratch::new("bash-input");
    let path = scratch.write("q.jsonl", &(lines.join("\n") + "\n"));
    conversation::read(&s(&path))
}

fn read_str(lines: &[&str]) -> Conversation {
    read(&lines.iter().map(|l| (*l).to_string()).collect::<Vec<_>>())
}

fn users(c: &Conversation) -> Vec<String> {
    c.items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::User { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn phone(lines: &[&str]) -> Vec<ChatMessage> {
    let lines: Vec<String> = lines.iter().map(|l| (*l).to_string()).collect();
    chat::parse(&lines, true, "s-1", 50).messages
}

fn queued_texts(messages: &[ChatMessage]) -> Vec<&str> {
    messages
        .iter()
        .filter(|m| m.queued)
        .map(|m| m.text.as_str())
        .collect()
}

#[test]
fn a_bash_input_taken_from_the_queue_is_not_left_queued() {
    let c = read(&fixture_lines());
    assert_eq!(c.queued, Vec::<String>::new());
    assert_eq!(users(&c).last().map(String::as_str), Some("! git branch"));
}

#[test]
fn a_bash_input_alone_in_the_queue_is_taken_too() {
    let c = read_str(&[ENQUEUE_BASH, DEQUEUE, TURN_END, BASH_INPUT, BASH_OUTPUT]);
    assert_eq!(c.queued, Vec::<String>::new());
    assert_eq!(users(&c), ["! git branch"]);
}

#[test]
fn the_bash_input_names_its_entry_when_an_older_prompt_is_also_queued() {
    let c = read_str(&[
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-13T09:11:40.000Z","content":"fix the build"}"#,
        ENQUEUE_BASH,
        DEQUEUE,
        TURN_END,
        BASH_INPUT,
        BASH_OUTPUT,
    ]);
    assert_eq!(c.queued, ["fix the build"]);
}

#[test]
fn a_prompt_queued_after_a_bash_input_waits_until_it_is_taken() {
    let mut lines = fixture_lines();
    lines.push(ENQUEUE_NEXT.to_string());
    let c = read(&lines);
    assert_eq!(c.queued, ["and then fix the build"]);

    lines.push(DEQUEUE.to_string());
    lines.push(
        r#"{"type":"user","message":{"role":"user","content":"and then fix the build"},"timestamp":"2026-11-13T09:12:11.000Z"}"#
            .to_string(),
    );
    let c = read(&lines);
    assert_eq!(c.queued, Vec::<String>::new());
    assert_eq!(
        users(&c).last().map(String::as_str),
        Some("and then fix the build")
    );
}

#[test]
fn a_bash_input_nobody_took_stays_queued_until_it_is() {
    let c = read_str(&[ENQUEUE_BASH]);
    assert_eq!(c.queued, ["git branch"]);
}

#[test]
fn a_queued_bash_input_that_was_removed_or_popped_is_gone() {
    for operation in ["remove", "popAll"] {
        let c = read_str(&[
            ENQUEUE_BASH,
            &format!(
                r#"{{"type":"queue-operation","operation":"{operation}","timestamp":"2026-11-13T09:11:55.000Z","content":"git branch"}}"#
            ),
        ]);
        assert_eq!(c.queued, Vec::<String>::new(), "{operation}");
        assert!(users(&c).is_empty(), "{operation}");
    }
}

#[test]
fn the_phone_chat_view_leaves_a_taken_bash_input_unqueued() {
    let messages = phone(&[ENQUEUE_BASH, DEQUEUE, TURN_END, BASH_INPUT, BASH_OUTPUT]);
    assert_eq!(queued_texts(&messages), Vec::<&str>::new());
    let shell: Vec<_> = messages
        .iter()
        .filter(|m| m.kind == ChatKind::Shell)
        .collect();
    assert_eq!(shell.len(), 1);
    assert_eq!(shell[0].text, "!git branch");
    assert_eq!(shell[0].output.as_deref(), Some("* main"));
}

#[test]
fn the_phone_chat_view_clears_a_bash_input_taken_behind_an_older_prompt() {
    let messages = phone(&[
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-13T09:11:40.000Z","content":"fix the build"}"#,
        ENQUEUE_BASH,
        DEQUEUE,
        TURN_END,
        BASH_INPUT,
        BASH_OUTPUT,
    ]);
    assert!(
        !queued_texts(&messages).contains(&"git branch"),
        "{messages:?}"
    );
}

#[test]
fn the_phone_chat_view_keeps_a_prompt_queued_after_a_bash_input() {
    let messages = phone(&[
        ENQUEUE_BASH,
        DEQUEUE,
        TURN_END,
        BASH_INPUT,
        BASH_OUTPUT,
        ENQUEUE_NEXT,
    ]);
    assert_eq!(queued_texts(&messages), ["and then fix the build"]);
}

#[test]
fn the_phone_chat_view_drops_a_queued_message_the_user_pulled_back() {
    let messages = phone(&[
        ENQUEUE_NEXT,
        r#"{"type":"queue-operation","operation":"popAll","timestamp":"2026-11-13T09:12:15.000Z","content":"and then fix the build"}"#,
    ]);
    assert_eq!(queued_texts(&messages), Vec::<&str>::new());
    assert!(messages.is_empty(), "{messages:?}");
}
