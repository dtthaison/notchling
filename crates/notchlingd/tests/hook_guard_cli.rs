#![cfg(unix)]

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    let dir = PathBuf::from(format!("/tmp/nl-hg-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("root/notchling/guard")).unwrap();
    Dir(dir)
}

fn guard_hook(dir: &Path, stdin: &[u8]) -> (Output, Duration) {
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_notchling"))
        .args(["hook", "--guard"])
        .env("NOTCHLING_DATA_ROOT", dir.join("root"))
        .env("HOME", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(stdin);
    let out = child.wait_with_output().unwrap();
    (out, started.elapsed())
}

fn passed(out: &Output) -> bool {
    out.status.code() == Some(0) && out.stdout.is_empty()
}

fn event(command: &str) -> Vec<u8> {
    json!({"session_id": "s1", "hook_event_name": "PreToolUse", "tool_name": "Bash",
        "tool_use_id": "toolu_1", "cwd": "/work", "tool_input": {"command": command}})
    .to_string()
    .into_bytes()
}

fn queue(dir: &Path) -> String {
    let mut paths: Vec<_> = std::fs::read_dir(dir.join("root/notchling/guard/spool"))
        .map(|e| e.flatten().map(|e| e.path()).collect::<Vec<_>>())
        .unwrap_or_default();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect()
}

#[test]
fn with_no_policy_the_hook_prints_nothing_and_records_nothing() {
    let dir = scratch("none");
    let (out, took) = guard_hook(&dir.0, &event("sleep 60"));
    assert!(passed(&out));
    assert!(took < Duration::from_secs(10));
    assert_eq!(queue(&dir.0), "");
}

#[test]
fn with_a_log_only_policy_the_hook_prints_nothing_and_queues_a_row() {
    let dir = scratch("log");
    std::fs::write(
        dir.0.join("root/notchling/guard/policy.json"),
        r#"{"v":1,"level":"log","cap":5}"#,
    )
    .unwrap();
    let (out, _) = guard_hook(&dir.0, &event("sleep 60"));
    assert!(passed(&out));
    let text = queue(&dir.0);
    assert!(!text.contains("sleep"));
    let row: Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(row["rule"], "poll");
    assert_eq!(row["session"], "s1");
}

#[test]
fn garbage_empty_and_huge_input_all_pass() {
    let dir = scratch("junk");
    std::fs::write(
        dir.0.join("root/notchling/guard/policy.json"),
        r#"{"v":1,"level":"log","cap":5}"#,
    )
    .unwrap();
    for stdin in [
        Vec::new(),
        b"{".to_vec(),
        b"\xff\xfe\x00".to_vec(),
        vec![b'x'; 3 * 1024 * 1024],
    ] {
        let (out, took) = guard_hook(&dir.0, &stdin);
        assert!(passed(&out));
        assert!(took < Duration::from_secs(10));
    }
    assert_eq!(queue(&dir.0), "");
}

#[test]
fn an_unknown_guard_flag_still_passes() {
    let dir = scratch("flag");
    let out = Command::new(env!("CARGO_BIN_EXE_notchling"))
        .args(["hook", "--guard", "--bogus"])
        .env("NOTCHLING_DATA_ROOT", dir.0.join("root"))
        .env("HOME", &dir.0)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(passed(&out));
}
