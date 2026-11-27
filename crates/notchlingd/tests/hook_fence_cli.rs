#![cfg(unix)]

mod common;

use common::SpawnRetrying;
use notchling_engine::cluster::fence::{FenceFile, Gate, FENCE_VERSION, WAIT_MAX_MS};
use notchling_engine::cluster::id::{fingerprint, NodeId};
use notchling_engine::cluster::members::Term;
use notchling_platform::clock::{boot_id, boot_ms};
use serde_json::{json, Value};
use std::collections::BTreeMap;
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
    let dir = PathBuf::from(format!("/tmp/nl-hf-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("root/notchling/cluster")).unwrap();
    Dir(dir)
}

fn node(name: &str) -> NodeId {
    NodeId::from_fingerprint(&fingerprint(name.as_bytes())).unwrap()
}

fn write_fence(dir: &Path, gate: Gate, boot: &str, valid_until: u64) {
    let mut sessions = BTreeMap::new();
    sessions.insert("s1".to_string(), gate);
    let f = FenceFile {
        v: FENCE_VERSION,
        term: Term {
            n: 1,
            node: node("m5"),
        },
        node: node("m5"),
        boot_id: boot.to_string(),
        valid_until,
        sessions,
    };
    std::fs::write(
        dir.join("root/notchling/cluster/fence.json"),
        serde_json::to_vec(&f).unwrap(),
    )
    .unwrap();
}

fn event(session: &str) -> Vec<u8> {
    json!({"session_id": session, "hook_event_name": "PreToolUse", "tool_name": "Bash",
        "tool_input": {"command": "ls"}})
    .to_string()
    .into_bytes()
}

fn fence_hook(dir: &Path, stdin: &[u8]) -> (Output, Duration) {
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_notchling"))
        .args(["hook", "--fence"])
        .env("NOTCHLING_DATA_ROOT", dir.join("root"))
        .env("HOME", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_retrying();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    (out, started.elapsed())
}

fn passed(out: &Output) -> bool {
    out.status.code() == Some(0) && out.stdout.is_empty()
}

fn reason(out: &Output) -> String {
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let v: Value = serde_json::from_slice(&out.stdout).expect("deny json");
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["hookEventName"], "PreToolUse");
    assert_eq!(h["permissionDecision"], "deny");
    h["permissionDecisionReason"].as_str().unwrap().to_string()
}

fn nothing_else_was_made(dir: &Path) {
    let mut names: Vec<String> = std::fs::read_dir(dir.join("root/notchling"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["cluster"], "the hook made nothing of its own");
}

#[test]
fn hook_denies_on_expired_fence_without_socket() {
    let dir = scratch("expired");
    let boot = boot_id().expect("a boot id");
    write_fence(&dir.0, Gate::Lease, &boot, 0);
    let (out, took) = fence_hook(&dir.0, &event("s1"));
    println!("notchling hook --fence, expired lease: {took:?}");
    assert!(reason(&out).contains("paused to keep it from running on two machines"));
    assert!(out.stderr.is_empty(), "{out:?}");
    assert!(
        took >= Duration::from_millis(WAIT_MAX_MS - 500),
        "it waited the whole {WAIT_MAX_MS} ms before denying: {took:?}"
    );
    assert!(took < Duration::from_secs(27), "{took:?}");
    nothing_else_was_made(&dir.0);
}

#[test]
fn a_lease_that_arrives_while_the_hook_waits_passes() {
    let dir = scratch("arrives");
    let boot = boot_id().expect("a boot id");
    write_fence(&dir.0, Gate::Lease, &boot, 0);
    let root = dir.0.clone();
    let renew = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1_200));
        let until = boot_ms().unwrap() + 3_600_000;
        write_fence(&root, Gate::Lease, &boot_id().unwrap(), until);
    });
    let (out, took) = fence_hook(&dir.0, &event("s1"));
    renew.join().unwrap();
    assert!(passed(&out), "{out:?}");
    assert!(took >= Duration::from_millis(1_000), "{took:?}");
    assert!(took < Duration::from_secs(8), "{took:?}");
}

#[test]
fn a_moved_session_is_denied_at_once_naming_the_machine() {
    let dir = scratch("moved");
    write_fence(&dir.0, Gate::Moved { to: "m1".into() }, "whatever", 0);
    let (out, took) = fence_hook(&dir.0, &event("s1"));
    assert_eq!(reason(&out), "This session continues on m1. Open it there.");
    assert!(took < Duration::from_secs(5), "{took:?}");
    nothing_else_was_made(&dir.0);
}

#[test]
fn no_file_a_valid_lease_free_and_other_sessions_pass_fast() {
    let dir = scratch("pass");
    let (out, took) = fence_hook(&dir.0, &event("s1"));
    assert!(passed(&out), "{out:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");

    let boot = boot_id().expect("a boot id");
    write_fence(&dir.0, Gate::Lease, &boot, boot_ms().unwrap() + 60_000);
    assert!(passed(&fence_hook(&dir.0, &event("s1")).0));
    assert!(passed(&fence_hook(&dir.0, &event("another")).0));
    write_fence(&dir.0, Gate::Free, "old-boot", 0);
    assert!(passed(&fence_hook(&dir.0, &event("s1")).0));
    nothing_else_was_made(&dir.0);
}

#[test]
fn odd_input_passes_with_exit_0_and_no_output() {
    let dir = scratch("odd");
    write_fence(&dir.0, Gate::Moved { to: "m1".into() }, "x", 0);
    for stdin in [
        &b""[..],
        b"not json",
        b"{}",
        br#"{"session_id": 3}"#,
        &[0xff, 0x00][..],
    ] {
        let (out, took) = fence_hook(&dir.0, stdin);
        assert!(passed(&out), "{stdin:?}: {out:?}");
        assert!(took < Duration::from_secs(5), "{took:?}");
    }
}

#[test]
fn the_fence_flag_takes_nothing_else() {
    let dir = scratch("args");
    write_fence(&dir.0, Gate::Moved { to: "m1".into() }, "x", 0);
    let mut child = Command::new(env!("CARGO_BIN_EXE_notchling"))
        .args(["hook", "--fence", "--socket", "/tmp/none.sock"])
        .env("NOTCHLING_DATA_ROOT", dir.0.join("root"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_retrying();
    let _ = child.stdin.take().unwrap().write_all(&event("s1"));
    let out = child.wait_with_output().unwrap();
    assert!(passed(&out), "{out:?}");
}

#[test]
fn a_stdin_that_never_closes_passes_inside_the_budget() {
    let dir = scratch("openstdin");
    write_fence(&dir.0, Gate::Moved { to: "m1".into() }, "x", 0);
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_notchling"))
        .args(["hook", "--fence"])
        .env("NOTCHLING_DATA_ROOT", dir.0.join("root"))
        .env("HOME", &dir.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_retrying();
    let held = child.stdin.take().unwrap();
    let out = child.wait_with_output().unwrap();
    drop(held);
    assert!(passed(&out), "{out:?}");
    let took = started.elapsed();
    assert!(took >= Duration::from_secs(4), "{took:?}");
    assert!(took < Duration::from_secs(12), "{took:?}");
}
