use super::*;
use serde_json::json;
use std::path::PathBuf;

const FIXTURE: &str = include_str!("../../tests/fixtures/ask/agy-permission.json");
const BINARY: &str = "/Applications/Notchling.app/Contents/MacOS/notchling";

fn binary() -> PathBuf {
    PathBuf::from(BINARY)
}

fn request(command: &str) -> Value {
    json!({"params": {"toolCall": {"kind": "execute", "rawInput": {"CommandLine": command}}}})
}

fn allowed(command: &str) -> bool {
    allows(&request(command), &binary())
}

#[test]
fn allows_exact_ask_wait() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    assert!(allows(&fixture, &binary()));
    assert!(allowed("notchling ask --wait -- 'Keep it?'"));
    assert!(allowed(&format!("{BINARY} ask --wait")));
    let args_only =
        json!({"params": {"toolCall": {"args": {"CommandLine": "notchling ask --wait -- x"}}}});
    assert!(allows(&args_only, &binary()));
}

#[test]
fn declines_ask_without_wait() {
    assert!(!allowed("notchling ask -- 'x'"));
    assert!(!allowed("notchling ask --json --wait -- x"));
    assert!(!allowed("notchling asks --wait"));
    assert!(!allowed("notchling answer --wait"));
    assert!(!allowed("notchling"));
    assert!(!allowed(""));
}

#[test]
fn declines_ask_after_an_operator() {
    assert!(!allowed("notchling ask --wait -- x; rm y"));
    assert!(!allowed("notchling ask --wait -- x && rm y"));
    assert!(!allowed("rm y; notchling ask --wait -- x"));
    assert!(!allowed("notchling ask --wait -- x | cat"));
    assert!(!allowed("notchling ask --wait -- x &"));
    assert!(!allowed("notchling ask --wait -- x\nrm y"));
    assert!(!allowed("notchling ask --wait -- $(rm y)"));
}

#[test]
fn declines_a_notchling_from_another_folder() {
    assert!(!allowed("/tmp/notchling ask --wait -- x"));
    assert!(!allowed("./notchling ask --wait -- x"));
    assert!(!allowed(
        "/Applications/Other.app/Contents/MacOS/notchling ask --wait -- x"
    ));
}

#[test]
fn declines_a_redirection() {
    assert!(!allowed("notchling ask --wait -- x > /tmp/a"));
    assert!(!allowed("notchling ask --wait -- x >> /tmp/a"));
    assert!(!allowed("notchling ask --wait -- x < /tmp/a"));
    assert!(!allowed("notchling ask --wait -- x 2>&1"));
    assert!(!allowed("notchling ask --wait -- x &> /tmp/a"));
    assert!(!allowed("notchling ask --wait -- x 2> /tmp/a"));
    assert!(!allowed("notchling ask --wait -- x>/tmp/a"));
    assert!(!allowed("notchling ask --wait -- x & rm y"));
    assert!(!allowed("notchling ask --wait -- x <(rm y)"));
    assert!(allowed("notchling ask --wait -- 'a > b & c'"));
    assert!(allowed("notchling ask --wait -- \"a -> b\""));
}

#[test]
fn reads_raw_input_not_title() {
    let title_only = json!({"params": {"toolCall": {"title": "notchling ask --wait -- x"}}});
    assert!(!allows(&title_only, &binary()));
    let differing = json!({"params": {"toolCall": {
        "title": "notchling ask --wait -- x",
        "rawInput": {"CommandLine": "rm -rf /"}}}});
    assert!(!allows(&differing, &binary()));
    let other_way = json!({"params": {"toolCall": {
        "title": "rm -rf /",
        "rawInput": {"CommandLine": "notchling ask --wait -- x"}}}});
    assert!(allows(&other_way, &binary()));
}
