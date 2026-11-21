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
    let _daemon = Daemon::start(&root, &home);

    for args in [
        vec!["inbox", "--json"],
        vec!["accounts", "--json"],
        vec!["limits", "--json"],
        vec!["orgs", "--json"],
        vec!["search", "anything", "--json"],
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
    let _daemon = Daemon::start(&root, &home);

    let (code, out, _) = run(&root, &home, &["limits", "--tool", "codex", "--json"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out["error"]["code"], "failed");
    assert_eq!(out["error"]["result"]["reason"], "no_session");

    let (code, out, _) = run(&root, &home, &["limits", "--tool", "nope", "--json"]);
    assert_eq!(code, 2, "{out}");

    let (code, out, _) = run(&root, &home, &["limits", "--pool", "x", "--json"]);
    assert_eq!(code, 2, "{out}");
}

#[test]
fn the_public_reads_work_for_a_shell() {
    let dir = scratch("reads-e2e-public");
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let _daemon = Daemon::start(&root, &home);

    let (code, out, err) = run(&root, &home, &["switches", "--json"]);
    assert_eq!(code, 0, "{out} {err}");
    for name in ["acting", "bridge", "accounts", "vault", "install", "cluster"] {
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
