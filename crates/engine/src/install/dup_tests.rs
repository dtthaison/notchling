use super::file::{apply, plan};
use super::hooks::{detect, plan_hooks};
use super::testutil::*;
use std::fs;

const OURS: &str = "'/opt/notchling/bin/notchling' hook";
const USERS: &str = "~/bin/users-hook.sh";

fn files() -> Vec<(&'static str, String)> {
    let entry = |extra: &str| {
        format!(
            r#"{{"hooks": {{"PreToolUse": [{{"matcher": "AskUserQuestion", "hooks": [{{"type": "command", "command": "{OURS}", "timeout": 300{extra}}}]}}]}}}}"#
        )
    };
    vec![
        (
            "group hooks twice",
            format!(
                r#"{{"hooks": {{"PreToolUse": [{{"matcher": "AskUserQuestion",
                  "hooks": [{{"type": "command", "command": "{OURS}", "timeout": 300}}],
                  "hooks": [{{"type": "command", "command": "{USERS}"}}]}}]}}}}"#
            ),
        ),
        (
            "entry command twice",
            format!(
                r#"{{"hooks": {{"PreToolUse": [{{"matcher": "AskUserQuestion", "hooks": [
                  {{"type": "command", "command": "{OURS}", "command": "{USERS}", "timeout": 300}}]}}]}}}}"#
            ),
        ),
        (
            "group matcher twice",
            format!(
                r#"{{"hooks": {{"PreToolUse": [{{"matcher": "AskUserQuestion", "matcher": "Bash",
                  "hooks": [{{"type": "command", "command": "{OURS}", "timeout": 300}}]}}]}}}}"#
            ),
        ),
        ("entry type twice", entry(r#", "type": "prompt""#)),
        ("entry timeout twice", entry(r#", "timeout": 1"#)),
        (
            "event twice",
            format!(
                r#"{{"hooks": {{"Stop": [], "PreToolUse": [{{"matcher": "AskUserQuestion", "hooks": [{{"type": "command", "command": "{OURS}", "timeout": 300}}]}}], "Stop": []}}}}"#
            ),
        ),
        (
            "hooks twice at the top",
            format!(
                r#"{{"hooks": {{"PreToolUse": [{{"matcher": "AskUserQuestion", "hooks": [{{"type": "command", "command": "{OURS}", "timeout": 300}}]}}]}}, "hooks": {{}}}}"#
            ),
        ),
        (
            "a key twice in an unrelated hook",
            format!(
                r#"{{"hooks": {{"Stop": [{{"hooks": [{{"type": "command", "command": "{USERS}", "async": true, "async": false}}]}}]}}}}"#
            ),
        ),
    ]
}

#[test]
fn a_repeated_key_under_hooks_is_refused_for_every_choice() {
    for (name, text) in files() {
        for (what, wanted) in [("install", install_all()), ("remove", remove_all())] {
            let err = plan_hooks(&text, &wanted)
                .err()
                .unwrap_or_else(|| panic!("{name}: {what} was planned"));
            assert!(err.0.contains("left as it is"), "{name}: {err}");
        }
        assert!(detect(&text).is_err(), "{name}: detect read it");
    }
}

#[test]
fn a_refused_file_is_never_written_and_gets_no_backup() {
    for (name, text) in files() {
        for (what, wanted) in [("install", install_all()), ("remove", remove_all())] {
            let dir = Scratch::new("dup");
            let path = dir.path("settings.json");
            fs::write(&path, &text).unwrap();
            assert!(plan(&path, &wanted).is_err(), "{name}: {what} planned");
            assert!(apply(&path, &wanted, 1).is_err(), "{name}: {what} applied");
            assert_eq!(fs::read(&path).unwrap(), text.as_bytes(), "{name}: {what}");
            assert_eq!(dir.names(), vec!["settings.json"], "{name}: {what}");
        }
    }
}

#[test]
fn the_same_key_in_different_objects_is_fine() {
    let text = format!(
        r#"{{"env": {{"A": 1}}, "hooks": {{"PreToolUse": [{{"matcher": "Bash", "hooks": [{{"type": "command", "command": "{USERS}"}}]}}, {{"matcher": "Edit", "hooks": [{{"type": "command", "command": "{USERS}"}}]}}]}}}}"#
    );
    let out = plan_hooks(&text, &install_all()).unwrap();
    assert!(out.changed);
    let text = r#"{"env": {"A": 1, "A": 2}, "model": "x", "model": "y"}"#;
    assert!(plan_hooks(text, &install_all()).unwrap().changed);
}
