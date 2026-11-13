use serde_json::{json, Value};

use super::hook_rows::*;
use crate::welcome::facts::HookFacts;
use crate::welcome::hooks::DETACHED;

fn entry(event: &str, matcher: &str, origin: &str, managed: bool) -> Value {
    json!({"event": event, "matcher": matcher, "origin": origin,
        "command": "'/Users/someone/secret-folder/notchling' hook", "managed": managed})
}

fn facts(state: &str, refused: Option<&str>, present: Vec<Value>) -> HookFacts {
    HookFacts::parse(&json!({"on": false, "applied": false,
        "plan": {"blocked": null, "skills": [],
            "hooks": {"state": state, "refused": refused, "changes": [], "notes": [],
                      "present": present}}}))
    .unwrap()
}

fn ours() -> Vec<Value> {
    vec![
        entry("PermissionRequest", "*", "notchling", true),
        entry("PreToolUse", "AskUserQuestion", "notchling", true),
        entry("PreToolUse", "Artifact", "notchling", true),
        entry("PostToolUse", "Artifact", "notchling", true),
        entry("PostToolUseFailure", "Artifact", "notchling", true),
    ]
}

fn titles(shown: &HooksShown) -> Vec<String> {
    shown.lines().into_iter().map(|l| l.title).collect()
}

#[test]
fn every_entry_is_a_row_with_what_it_does() {
    let shown = HooksShown::from_plan(Ok(facts("present", None, ours())));
    assert_eq!(
        titles(&shown),
        [
            "PermissionRequest · All tools",
            "PreToolUse · AskUserQuestion",
            "PreToolUse · Artifact",
            "PostToolUse · Artifact",
            "PostToolUseFailure · Artifact",
        ]
    );
    let notes: Vec<_> = shown.lines().into_iter().map(|l| l.note.unwrap()).collect();
    assert_eq!(
        notes,
        [
            "Asks for permission in the notch and Chat",
            "Shows Claude's questions in the notch and Chat",
            "Saves and finds published pages on this Mac",
            "Saves and finds published pages on this Mac",
            "Saves and finds published pages on this Mac",
        ]
    );
    assert!(shown.lines().iter().all(|l| l.tag.is_none()));
}

#[test]
fn swift_and_unmanaged_entries_are_told_apart() {
    let shown = HooksShown::from_plan(Ok(facts(
        "present",
        None,
        vec![
            entry("PermissionRequest", "*", "swift", false),
            entry("PostToolUse", "Artifact", "notchling", false),
            entry("PreToolUse", "AskUserQuestion", "notchling", true),
        ],
    )));
    let tags: Vec<_> = shown.lines().into_iter().map(|l| l.tag).collect();
    assert_eq!(tags, [Some("Swift app"), Some("Not managed"), None]);
}

#[test]
fn an_unknown_slot_still_gets_a_plain_line_and_a_cut_name() {
    let long = "X".repeat(100);
    let shown = HooksShown::from_plan(Ok(facts(
        "present",
        None,
        vec![entry("Stop", &long, "notchling", true)],
    )));
    let line = shown.lines().remove(0);
    assert_eq!(line.title, format!("Stop · {}", "X".repeat(40)));
    assert_eq!(line.note, Some("Runs Notchling's hook"));
    let empty_matcher = HooksShown::from_plan(Ok(facts(
        "present",
        None,
        vec![entry("PermissionRequest", "", "notchling", true)],
    )));
    assert_eq!(titles(&empty_matcher), ["PermissionRequest · All tools"]);
}

#[test]
fn other_hooks_and_commands_never_show() {
    let shown = HooksShown::from_plan(Ok(facts(
        "present",
        None,
        vec![
            entry("Stop", "*", "other", false),
            entry("PermissionRequest", "*", "notchling", true),
        ],
    )));
    assert_eq!(titles(&shown), ["PermissionRequest · All tools"]);
    let all = format!("{:?}", shown.lines());
    assert!(!all.contains("secret-folder") && !all.contains("Stop"));
}

#[test]
fn no_entries_says_so_and_points_to_welcome() {
    for state in ["missing", "empty", "present"] {
        let shown = HooksShown::from_plan(Ok(facts(state, None, vec![])));
        assert_eq!(shown, HooksShown::Empty);
    }
    let line = HooksShown::Empty.lines().remove(0);
    assert_eq!(line.title, "No hooks installed");
    assert_eq!(line.note, Some("Use Install Hook in Welcome to add them."));
}

#[test]
fn a_detached_daemon_shows_the_existing_line() {
    let shown = HooksShown::from_plan(Err(DETACHED.to_owned()));
    assert_eq!(shown, HooksShown::Detached);
    assert_eq!(titles(&shown), [DETACHED]);
}

#[test]
fn a_daemon_error_shows_its_message() {
    let shown = HooksShown::from_plan(Err("the daemon's plan isn't one this app reads".into()));
    assert_eq!(
        titles(&shown),
        ["the daemon's plan isn't one this app reads"]
    );
}

#[test]
fn a_refused_file_shows_one_category_and_none_of_its_text() {
    let cases = [
        (
            "settings.json is not valid JSON (expected a value at line 3: SECRET-TOKEN); it was left as it is",
            "isn't valid JSON",
        ),
        (
            "settings.json does not hold a JSON object; it was left as it is",
            "isn't a JSON object",
        ),
        (
            "settings.json has \"hooks\" twice; it was left as it is",
            "has a key written twice",
        ),
        (
            "\"hooks\" is not an object; it was left as it is",
            "has hooks that aren't an object",
        ),
        (
            "\"Stop\" is not a list; settings.json was left as it is",
            "has a hook event that isn't a list",
        ),
        (
            "settings.json is a link to a file that is not there; it was left alone",
            "is a link to a missing file",
        ),
        (
            "settings.json is not UTF-8 text; it was left alone",
            "isn't UTF-8 text",
        ),
        ("something new", "can't be read"),
    ];
    for (reason, category) in cases {
        let shown = HooksShown::from_plan(Ok(facts("refused", Some(reason), vec![])));
        let lines = shown.lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].title,
            format!("Claude Code's settings.json was left alone: it {category}.")
        );
        assert!(!lines[0].title.contains("SECRET-TOKEN"));
    }
    let none = HooksShown::from_plan(Ok(facts("refused", None, vec![])));
    assert!(titles(&none)[0].ends_with("can't be read."));
}

#[test]
fn checking_is_one_plain_line() {
    assert_eq!(titles(&HooksShown::Checking), [CHECKING]);
}
