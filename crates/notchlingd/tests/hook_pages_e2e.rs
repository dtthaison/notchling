//! `notchling hook` with the Artifact tool's events, as Claude Code runs it: the built binary, the
//! event on stdin, its stdout and exit code, over a temp data root and a fake `HOME`. No daemon is
//! running and no real folder is read. A publish is saved where `notchling pages` finds it, a read
//! of the saved link is denied with the local copy, and everything else passes.
#![cfg(unix)]

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    let dir = PathBuf::from(format!("/tmp/nl-hp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    Dir(dir)
}

const LINK: &str = "https://claude.ai/artifact/AAAAAAAAAAAAAAAAAAAAAA";

fn notchling(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_notchling"));
    c.args(args)
        .env("NOTCHLING_DATA_ROOT", dir.join("root"))
        .env("HOME", dir)
        .env_remove("CLAUDE_CONFIG_DIR");
    c
}

fn hook(dir: &Path, stdin: &[u8]) -> Output {
    // The socket is a name nobody listens on: pages events never ask for it.
    let socket = dir.join("missing.sock");
    let mut child = notchling(dir, &["hook", "--socket", socket.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stderr.is_empty(), "a hook says nothing on stderr");
    out
}

fn say(dir: &Path, event: &Value) -> Option<Value> {
    let out = hook(dir, &serde_json::to_vec(event).unwrap());
    (!out.stdout.is_empty()).then(|| serde_json::from_slice(&out.stdout).unwrap())
}

fn publish(dir: &Path) -> Value {
    json!({
        "session_id": "s1",
        "hook_event_name": "PostToolUse",
        "tool_name": "Artifact",
        "cwd": dir.join("work"),
        "tool_input": {"file_path": "probe.html"},
        "tool_response": format!("Published probe.html to {LINK} (version id 1-aaaa)"),
    })
}

fn read(event: &str, url: &str) -> Value {
    json!({
        "session_id": "s1",
        "hook_event_name": event,
        "tool_name": "Artifact",
        "cwd": "/w",
        "tool_input": {"action": "read", "url": url},
    })
}

#[test]
fn a_publish_is_saved_and_a_read_of_its_link_is_answered_from_the_copy() {
    let dir = scratch("flow");
    std::fs::write(
        dir.0.join("work/probe.html"),
        "<title>pages-hook-probe</title><p>hello</p>",
    )
    .unwrap();
    // Under a fixture root the root is read as `~`.
    std::fs::create_dir_all(dir.0.join("root")).unwrap();
    std::fs::write(
        dir.0.join("root/.claude.json"),
        r#"{"oauthAccount":{"emailAddress":"me@example.com"}}"#,
    )
    .unwrap();

    // The publish is recorded, silently, with no daemon.
    assert_eq!(say(&dir.0, &publish(&dir.0)), None);
    let library = dir.0.join("root/notchling/pages");
    let list: Value =
        serde_json::from_slice(&std::fs::read(library.join("pages.json")).unwrap()).unwrap();
    assert_eq!(list["pages"].as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list["pages"][0]["account"], "me@example.com");

    // `notchling pages` reads the same folder.
    let found = notchling(&dir.0, &["pages", "pages-hook-probe"])
        .output()
        .unwrap();
    assert_eq!(found.status.code(), Some(0), "{found:?}");
    assert!(String::from_utf8_lossy(&found.stdout).contains("AAAAAAAAAAAAAAAAAAAAAA"));

    // A whole-page read of the link is denied with the local copy's path.
    let out = say(&dir.0, &read("PreToolUse", LINK)).unwrap();
    let h = &out["hookSpecificOutput"];
    assert_eq!(h["permissionDecision"], "deny");
    let reason = h["permissionDecisionReason"].as_str().unwrap();
    let folder = list["pages"][0]["folder"].as_str().unwrap();
    assert!(
        reason.contains(&format!(
            "Local copy: {}",
            library.join(folder).join("index.html").display()
        )),
        "{reason}"
    );

    // A read of a link we don't keep passes; a failed read of the kept one gets the fallback.
    assert_eq!(
        say(
            &dir.0,
            &read(
                "PreToolUse",
                "https://claude.ai/artifact/BBBBBBBBBBBBBBBBBBBBBB"
            )
        ),
        None
    );
    let out = say(&dir.0, &read("PostToolUseFailure", LINK)).unwrap();
    assert!(out["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .starts_with("Notchling keeps a local copy"));
}

#[test]
fn other_events_and_odd_input_pass_and_make_no_library() {
    let dir = scratch("other");
    for event in [
        json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "ls"}}),
        json!({"hook_event_name": "PostToolUse", "tool_name": "Read", "tool_response": "404"}),
        json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_input": {}}),
    ] {
        assert_eq!(say(&dir.0, &event), None, "{event}");
    }
    for stdin in [
        &b"not json"[..],
        b"",
        b"[]",
        b"{\"tool_name\":\"Artifact\"}",
    ] {
        let out = hook(&dir.0, stdin);
        assert!(out.stdout.is_empty(), "{stdin:?}");
    }
    assert!(!dir.0.join("root/notchling/pages").exists());
}
