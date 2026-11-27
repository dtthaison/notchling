mod common;

use common::*;
use notchling_engine::transcript::finished::WINDOW;
use notchling_engine::transcript::shell_titles::ShellTitles;
use notchling_engine::transcript::tail::RAW_BYTES;
use std::fs;
use std::io::Write;
use std::sync::atomic::Ordering;

const SUB: &str = include_str!("fixtures/shell-titles/subagents/agent-a1.jsonl");
const PAD: &str = r#"{"type":"user","message":{"role":"user","content":"padding padding padding padding padding padding padding padding padding padding padding padding"}}"#;

fn wanted(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

#[test]
fn a_big_file_is_read_from_its_tail_window_and_a_huge_partial_line_is_not_reread() {
    let raw = || RAW_BYTES.load(Ordering::Relaxed);
    let scratch = Scratch::new("shell-sub-bounded");
    let mut text = String::from(SUB);
    let pad = format!("{PAD}\n");
    while (text.len() as u64) < WINDOW + 1_000_000 {
        text.push_str(&pad);
    }
    let file = scratch.write("subagents/agent-a1.jsonl", &text);
    let dir = scratch.path("subagents");
    let mut titles = ShellTitles::default();

    let before = raw();
    let found = titles.read(&dir, &wanted(&["bsub0001"]), 0.0);
    assert!(
        found.is_empty(),
        "a launch before the tail window is not seen"
    );
    assert!(raw() - before <= WINDOW, "read {} bytes", raw() - before);

    let mut handle = fs::OpenOptions::new().append(true).open(&file).unwrap();
    handle.write_all(&vec![b'x'; 1_000_000]).unwrap();
    let _ = titles.read(&dir, &wanted(&["bsub0001"]), 0.0);
    let before = raw();
    let _ = titles.read(&dir, &wanted(&["bsub0001"]), 0.0);
    assert!(
        raw() - before <= 1_000,
        "an oversized partial line was read again: {} bytes",
        raw() - before
    );

    writeln!(handle, "yy").unwrap();
    writeln!(
        handle,
        r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"toolu_z","name":"Bash","input":{{"command":"sleep 1","description":"After the long line","run_in_background":true}}}}]}}}}"#
    )
    .unwrap();
    writeln!(
        handle,
        r#"{{"type":"user","message":{{"role":"user","content":[{{"tool_use_id":"toolu_z","type":"tool_result","content":"Command running in background with ID: bafter01. Output is being written to: /private/tmp/x/tasks/bafter01.output","is_error":false}}]}}}}"#
    )
    .unwrap();
    let found = titles.read(&dir, &wanted(&["bafter01"]), 0.0);
    assert_eq!(
        found.get("bafter01").map(String::as_str),
        Some("After the long line")
    );
}
