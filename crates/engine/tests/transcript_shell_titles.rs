mod common;

use common::*;
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::shell_titles::ShellTitles;
use std::fs;
use std::io::Write;
use std::path::Path;

const MAIN: &str = include_str!("fixtures/shell-titles/main.jsonl");
const SUB: &str = include_str!("fixtures/shell-titles/subagents/agent-a1.jsonl");

fn wanted(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

#[test]
fn a_background_bash_call_with_a_description_gives_a_shell_titled_by_it() {
    let scratch = Scratch::new("shell-title");
    let path = s(&scratch.write("main.jsonl", MAIN));
    let read = conversation::read(&path);
    let shell = read
        .shells
        .iter()
        .find(|sh| sh.id == "bmain0001")
        .expect("shell");
    assert_eq!(shell.title, "Wait for the merge tests to finish");
    assert_eq!(shell.description, "Wait for the merge tests to finish");
    assert!(shell.command.starts_with("tail -f /tmp/b2m4-test.log"));
}

#[test]
fn a_background_bash_call_without_a_description_falls_back_to_the_command() {
    let scratch = Scratch::new("shell-no-title");
    let path = s(&scratch.write("main.jsonl", MAIN));
    let read = conversation::read(&path);
    let shell = read
        .shells
        .iter()
        .find(|sh| sh.id == "bmain0002")
        .expect("shell");
    assert_eq!(shell.description, "");
    assert_eq!(shell.title, "Ran sleep 600");
    assert_eq!(shell.command, "sleep 600");
}

#[test]
fn a_shell_a_subagent_started_is_titled_from_the_subagent_transcript() {
    let scratch = Scratch::new("shell-sub-title");
    scratch.write("subagents/agent-a1.jsonl", SUB);
    let mut titles = ShellTitles::default();
    let found = titles.read(&scratch.path("subagents"), &wanted(&["bsub0001"]), 0.0);
    assert_eq!(
        found.get("bsub0001").map(String::as_str),
        Some("Review phase 4b with Hydrogen")
    );
    assert_eq!(found.len(), 1, "a call that started no shell has no entry");
}

#[test]
fn a_launch_appended_later_is_found_without_rereading_the_start() {
    let scratch = Scratch::new("shell-sub-grow");
    let file = scratch.write("subagents/agent-a1.jsonl", SUB);
    let mut titles = ShellTitles::default();
    assert!(titles
        .read(&scratch.path("subagents"), &wanted(&["bsub0002"]), 0.0)
        .is_empty());
    let more = [
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_s9","name":"Bash","input":{"command":"sleep 9","description":"Wait for the render","run_in_background":true}}]}}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_s9","type":"tool_result","content":"Command running in background with ID: bsub0002. Output is being written to: /private/tmp/claude-501/-work/s1/tasks/bsub0002.output","is_error":false}]}}"#,
    ];
    let mut handle = fs::OpenOptions::new().append(true).open(&file).unwrap();
    for line in more {
        writeln!(handle, "{line}").unwrap();
    }
    let found = titles.read(&scratch.path("subagents"), &wanted(&["bsub0002"]), 0.0);
    assert_eq!(
        found.get("bsub0002").map(String::as_str),
        Some("Wait for the render")
    );
}

#[test]
fn a_missing_folder_or_a_file_older_than_the_shell_gives_nothing() {
    let scratch = Scratch::new("shell-sub-none");
    let mut titles = ShellTitles::default();
    assert!(titles
        .read(
            Path::new("/nonexistent/subagents"),
            &wanted(&["bsub0001"]),
            0.0
        )
        .is_empty());
    scratch.write("subagents/agent-a1.jsonl", SUB);
    assert!(titles
        .read(
            &scratch.path("subagents"),
            &wanted(&["bsub0001"]),
            4_102_444_800.0
        )
        .is_empty());
}

const MOVED_CALL: &str = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_mv","name":"Bash","input":{"command":"sleep 700","description":"Wait for the slow build"}}]}}"#;
const MOVED_RESULT: &str = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_mv","type":"tool_result","content":"Command was moved to the background (ID: bmoved01) after running for 180s. Output is being written to: /private/tmp/claude-501/-work/s1/tasks/bmoved01.output. You will be notified when it completes.","is_error":false}]}}"#;

#[test]
fn a_command_the_harness_moved_to_the_background_is_titled_too() {
    let scratch = Scratch::new("shell-sub-moved");
    scratch.write(
        "subagents/agent-a1.jsonl",
        &format!("{MOVED_CALL}\n{MOVED_RESULT}\n"),
    );
    let mut titles = ShellTitles::default();
    let found = titles.read(&scratch.path("subagents"), &wanted(&["bmoved01"]), 0.0);
    assert_eq!(
        found.get("bmoved01").map(String::as_str),
        Some("Wait for the slow build")
    );
}
