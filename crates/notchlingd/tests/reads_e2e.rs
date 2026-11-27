#![cfg(unix)]

mod common;

use common::{home, notchling, scratch, Daemon};
use serde_json::Value;
use std::path::Path;

fn run(root: &Path, home: &Path, args: &[&str]) -> (i32, Value, String) {
    let (code, out, err) = notchling(root, home, args);
    let value = serde_json::from_str(&out).unwrap_or(Value::Null);
    (code, value, err)
}

#[test]
fn a_shell_gets_empty_scoped_reads_with_no_session_and_no_personal_data() {
    let dir = scratch("reads-e2e-shell");
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let _daemon = Daemon::start(&root, &home).warmed(&root);

    for args in [
        vec!["inbox", "--json"],
        vec!["accounts", "--json"],
        vec!["limits", "--json"],
        vec!["orgs", "--json"],
        vec!["sessions", "history", "--json"],
        vec!["search", "anything", "--json"],
        vec!["search", "anything", "--mode", "hybrid", "--json"],
        vec!["search", "anything", "--mode", "semantic", "--json"],
    ] {
        let (code, out, err) = run(&root, &home, &args);
        assert_eq!(code, 0, "{args:?} {out} {err}");
        assert_eq!(out["reason"], "no_session", "{args:?} {out}");
        assert!(!out.to_string().contains("personal"), "{args:?} {out}");
    }
}

#[test]
fn limits_for_a_tool_exits_1_for_a_shell_with_the_reason_in_the_error() {
    let dir = scratch("reads-e2e-limits");
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let _daemon = Daemon::start(&root, &home).warmed(&root);

    let (code, out, _) = run(&root, &home, &["limits", "--tool", "codex", "--json"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out["error"]["code"], "failed");
    assert_eq!(out["result"]["reason"], "no_session");

    let (code, out, _) = run(&root, &home, &["limits", "--tool", "nope", "--json"]);
    assert_eq!(code, 2, "{out}");

    let (code, out, _) = run(&root, &home, &["limits", "--pool", "x", "--json"]);
    assert_eq!(code, 2, "{out}");
}

#[test]
fn sessions_history_checks_its_flags_before_any_connection() {
    let dir = scratch("reads-e2e-history-flags");
    let root = dir.0.join("root");
    let home = home(&dir.0);
    for args in [
        vec!["sessions", "history", "--limit", "0", "--json"],
        vec!["sessions", "history", "--limit", "101", "--json"],
        vec!["sessions", "history", "--limit", "x", "--json"],
        vec!["sessions", "history", "--before", "--json"],
        vec!["sessions", "history", "extra", "--json"],
    ] {
        let (code, out, _) = run(&root, &home, &args);
        assert_eq!(code, 2, "{args:?} {out}");
    }
}

#[test]
fn the_public_reads_work_for_a_shell() {
    let dir = scratch("reads-e2e-public");
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let _daemon = Daemon::start(&root, &home).warmed(&root);

    let (code, out, err) = run(&root, &home, &["switches", "--json"]);
    assert_eq!(code, 0, "{out} {err}");
    for name in [
        "acting", "bridge", "accounts", "vault", "install", "cluster",
    ] {
        assert!(out.get(name).is_some(), "{name} in {out}");
    }

    let (code, out, err) = run(&root, &home, &["awake", "--json"]);
    assert_eq!(code, 0, "{out} {err}");
    assert!(out.is_object(), "{out}");

    let (code, out, err) = run(&root, &home, &["roles", "--json"]);
    assert_eq!(code, 0, "{out} {err}");

    let (code, out, err) = run(&root, &home, &["versions", "--json"]);
    assert_eq!(code, 0, "{out} {err}");
    assert!(out.get("installed").is_some(), "{out}");

    let (code, out, _) = run(&root, &home, &["awake", "maybe", "--json"]);
    assert_eq!(code, 2, "{out}");
}

#[test]
fn a_bad_argument_exits_2_even_with_no_daemon_and_a_good_one_says_not_running() {
    let dir = scratch("reads-e2e-nodaemon");
    let root = dir.0.join("root");
    let home = home(&dir.0);

    for args in [
        vec!["limits", "--tool", "nope"],
        vec!["limits", "--pool", "x"],
        vec!["limits", "--tool", "--json"],
        vec!["awake", "maybe"],
        vec!["search", "x", "--since", "NaN"],
        vec!["search", "x", "--kind", "folders"],
        vec!["search", "x", "--mode", "fuzzy"],
        vec!["search", "x", "--mode"],
    ] {
        let (code, out, err) = notchling(&root, &home, &args);
        assert_eq!(code, 2, "{args:?} {out} {err}");
    }

    let (code, out, err) = notchling(&root, &home, &["limits", "--json"]);
    assert_eq!(code, 3, "{out} {err}");
}
