mod common;

use common::*;
use notchling_engine::transcript::locate::{predecessor, successor};
use std::time::Duration;

fn tick() {
    std::thread::sleep(Duration::from_millis(30));
}

fn named(agent: &str) -> String {
    format!(r#"{{"type":"agent-name","agentName":"{agent}"}}"#) + "\n"
}

fn cleared(agent: &str) -> String {
    format!(r#"{{"type":"system","agentName":"{agent}","content":"SessionStart:clear"}}"#) + "\n"
}

#[test]
fn a_new_transcript_whose_clear_line_is_not_written_yet_finds_its_predecessor_once_it_is() {
    let scratch = Scratch::new("pending-before");
    let old = s(&scratch.write("p/old.jsonl", &named("worker")));
    tick();
    let new = s(&scratch.write("p/new.jsonl", &named("worker")));
    assert_eq!(predecessor(&new, "worker"), None, "no clear line yet");
    scratch.append("p/new.jsonl", &cleared("worker"));
    assert_eq!(
        predecessor(&new, "worker"),
        Some(old),
        "an append does not change the folder, and must not leave the miss cached"
    );
}

#[test]
fn an_empty_new_transcript_finds_its_predecessor_once_the_head_arrives() {
    let scratch = Scratch::new("pending-empty");
    let old = s(&scratch.write("p/old.jsonl", &named("worker")));
    tick();
    let new = s(&scratch.write("p/new.jsonl", ""));
    assert_eq!(predecessor(&new, "worker"), None);
    scratch.append("p/new.jsonl", &cleared("worker"));
    assert_eq!(predecessor(&new, "worker"), Some(old));
}

#[test]
fn a_new_transcript_whose_clear_line_is_not_written_yet_is_found_as_a_successor_once_it_is() {
    let scratch = Scratch::new("pending-after");
    let old = s(&scratch.write("p/old.jsonl", "{}\n"));
    tick();
    let new = s(&scratch.write("p/new.jsonl", &named("worker")));
    assert_eq!(successor(&old, "worker"), None, "no clear line yet");
    scratch.append("p/new.jsonl", &cleared("worker"));
    assert_eq!(successor(&old, "worker"), Some(new));
}

#[test]
fn a_transcript_with_a_long_head_and_no_clear_stays_without_a_predecessor() {
    let scratch = Scratch::new("pending-long");
    scratch.write("p/old.jsonl", &named("worker"));
    tick();
    let filler = "x".repeat(20_000);
    let new = s(&scratch.write(
        "p/new.jsonl",
        &(named("worker") + &format!("{{\"type\":\"user\",\"x\":\"{filler}\"}}\n")),
    ));
    assert_eq!(predecessor(&new, "worker"), None);
    scratch.append("p/new.jsonl", &cleared("worker"));
    assert_eq!(
        predecessor(&new, "worker"),
        None,
        "a clear line after the head window is not a clear start"
    );
}
