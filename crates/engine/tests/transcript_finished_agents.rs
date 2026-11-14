mod common;

use common::*;
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::finished::{self, WINDOW};
use notchling_engine::transcript::tail::{PARSED_BYTES, RAW_BYTES};
use std::sync::atomic::Ordering;
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

fn note(id: &str, status: &str) -> String {
    format!(
        r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-05T10:02:00.000Z","content":"<task-notification>\n<task-id>{id}</task-id>\n<status>{status}</status>\n</task-notification>"}}"#
    ) + "\n"
}

fn result(tool: &str, agent: &str, status: &str, error: bool) -> String {
    format!(
        r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{tool}","is_error":{error},"content":"x"}}]}},"toolUseResult":{{"status":"{status}","agentId":"{agent}"}},"timestamp":"2026-11-05T10:03:00.000Z"}}"#
    ) + "\n"
}

fn filler(bytes: usize) -> String {
    let line = format!(
        r#"{{"type":"assistant","message":{{"id":"m","role":"assistant","content":[{{"type":"text","text":"{}"}}]}}}}"#,
        "z".repeat(900)
    ) + "\n";
    line.repeat(bytes / line.len() + 1)
}

fn raw() -> u64 {
    RAW_BYTES.load(Ordering::Relaxed)
}

fn parsed() -> u64 {
    PARSED_BYTES.load(Ordering::Relaxed)
}

#[test]
fn it_finds_what_the_conversation_parser_finds() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let scratch = Scratch::new("finished-parity");
    let sidechain = result("t9", "side", "completed", false)
        .replace(r#""type":"user""#, r#""type":"user","isSidechain":true"#);
    let text = note("n1", "completed")
        + &note("n2", "killed")
        + &result("t1", "a1", "completed", false)
        + &result("t2", "a2", "completed", true)
        + &result("t3", "a3", "async_launched", false)
        + &sidechain
        + "not json at all\n";
    let path = s(&scratch.write("t.jsonl", &text));
    let expected = conversation::read(&path).finished_agents;
    assert_eq!(expected.len(), 6, "{expected:?}");
    assert_eq!(*finished::read(&path), expected);
}

#[test]
fn it_reads_a_file_once_and_again_only_when_it_changes() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let scratch = Scratch::new("finished-cache");
    let path = s(&scratch.write("t.jsonl", &note("n1", "completed")));
    let before = raw();
    let first = finished::read(&path);
    assert_eq!(first.get("n1").map(String::as_str), Some("completed"));
    let cost = raw() - before;
    assert!(cost > 0, "the first read scans the file");

    assert_eq!(finished::read(&path), first);
    assert_eq!(
        raw() - before,
        cost,
        "an unchanged file is not scanned again"
    );

    scratch.append("t.jsonl", &note("n2", "failed"));
    let grown = finished::read(&path);
    assert_eq!(grown.get("n2").map(String::as_str), Some("failed"));
    assert!(raw() - before > cost, "a grown file is scanned again");
}

#[test]
fn it_never_parses_a_conversation_and_reads_no_more_than_its_window() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let scratch = Scratch::new("finished-window");
    let text = note("old", "completed")
        + &filler(WINDOW as usize * 2)
        + &note("recent", "completed")
        + &filler(1000);
    let path = s(&scratch.write("t.jsonl", &text));
    let (raw_before, parsed_before) = (raw(), parsed());
    let found = finished::read(&path);
    assert_eq!(found.get("recent").map(String::as_str), Some("completed"));
    assert_eq!(
        found.get("old"),
        None,
        "what lies before the window is not looked at"
    );
    assert_eq!(
        parsed(),
        parsed_before,
        "the shared chat budget is not used"
    );
    let scanned = raw() - raw_before;
    assert!(scanned <= WINDOW, "{scanned} bytes scanned");
    assert!(scanned > WINDOW / 2, "{scanned} bytes scanned");
}

#[test]
fn a_missing_file_has_nothing_finished() {
    let scratch = Scratch::new("finished-missing");
    let path = s(&scratch.path("nope.jsonl"));
    assert!(finished::read(&path).is_empty());
}
