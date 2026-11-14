use notchling_locale as t;
use serde_json::{json, Value};

use super::agents::HOOKS;
use super::hook_rows::*;
use crate::welcome::facts::HookFacts;
use crate::welcome::hooks::DETACHED;

const COMMAND: &str = "'/Users/someone/secret-folder/notchling' hook";

fn entry(event: &str, matcher: &str, origin: &str, managed: bool) -> Value {
    json!({"event": event, "matcher": matcher, "origin": origin,
        "command": COMMAND, "managed": managed})
}

fn facts(state: &str, refused: Option<&str>, present: Vec<Value>) -> HookFacts {
    HookFacts::parse(&json!({"on": false, "applied": false,
        "plan": {"blocked": null, "skills": [],
            "hooks": {"state": state, "refused": refused, "changes": [], "notes": [],
                      "present": present}}}))
    .unwrap()
}

fn ours() -> Vec<Value> {
    HOOKS
        .iter()
        .map(|h| entry(h.event, h.matcher, "notchling", true))
        .collect()
}

fn shown(present: Vec<Value>) -> HooksShown {
    HooksShown::from_plan(Ok(facts("present", None, present)))
}

fn titles(shown: &HooksShown) -> Vec<String> {
    shown.lines().into_iter().map(|l| l.title).collect()
}

fn states(shown: &HooksShown) -> Vec<HookState> {
    shown.lines().into_iter().map(|l| l.state).collect()
}

#[test]
fn the_five_hooks_are_rows_with_their_text_and_command() {
    let shown = shown(ours());
    assert_eq!(
        titles(&shown),
        [
            "PermissionRequest · *",
            "PreToolUse · AskUserQuestion",
            "PreToolUse · Artifact",
            "PostToolUse · Artifact",
            "PostToolUseFailure · Artifact",
        ]
    );
    assert_eq!(states(&shown), [HookState::Installed; 5]);
    for (line, hook) in shown.lines().iter().zip(HOOKS) {
        assert_eq!(line.what, hook.what.get());
        assert_eq!(
            line.command.as_deref(),
            Some("'/Users/someone/secret-folder/notchling' hook")
        );
    }
    assert_eq!(shown.status(), None);
}

#[test]
fn a_hook_that_is_not_in_the_file_says_so_and_has_no_command() {
    let shown = shown(vec![entry("PreToolUse", "Artifact", "notchling", true)]);
    assert_eq!(
        states(&shown),
        [
            HookState::Missing,
            HookState::Missing,
            HookState::Installed,
            HookState::Missing,
            HookState::Missing,
        ]
    );
    let lines = shown.lines();
    assert_eq!(lines.len(), 5);
    assert!(lines[0].command.is_none() && lines[2].command.is_some());
    assert!(lines.iter().all(|l| l.what != t::settings::hooks_other()));
    let none = self::shown(vec![]);
    assert_eq!(states(&none), [HookState::Missing; 5]);
}

#[test]
fn swift_and_unmanaged_entries_are_told_apart() {
    let shown = shown(vec![
        entry("PermissionRequest", "*", "swift", false),
        entry("PostToolUse", "Artifact", "notchling", false),
        entry("PreToolUse", "AskUserQuestion", "notchling", true),
        entry("Stop", "*", "swift", false),
    ]);
    assert_eq!(
        states(&shown),
        [
            HookState::Swift,
            HookState::Installed,
            HookState::Missing,
            HookState::Missing,
            HookState::Missing,
            HookState::NotManaged,
            HookState::Swift,
        ]
    );
    let lines = shown.lines();
    assert_eq!(lines[0].command, None);
    assert_eq!(lines[5].title, "PostToolUse · Artifact");
    assert_eq!(lines[6].title, "Stop · *");
    assert_eq!(lines[6].what, t::settings::hooks_other());
}

#[test]
fn an_unknown_slot_still_gets_a_plain_line_and_a_cut_name() {
    let long = "X".repeat(100);
    let shown = shown(vec![entry("Stop", &long, "notchling", true)]);
    let line = shown.lines().remove(5);
    assert_eq!(line.title, format!("Stop · {}", "X".repeat(40)));
    assert_eq!(line.what, t::settings::hooks_other());
    let empty_matcher = shown_one(entry("PermissionRequest", "", "notchling", false));
    assert_eq!(empty_matcher.lines()[5].title, "PermissionRequest · *");
}

fn shown_one(e: Value) -> HooksShown {
    shown(vec![e])
}

#[test]
fn other_hooks_never_show() {
    let shown = shown(vec![
        entry("Stop", "*", "other", false),
        entry("PermissionRequest", "*", "notchling", true),
    ]);
    assert_eq!(shown.lines().len(), 5);
    assert_eq!(shown.lines()[0].state, HookState::Installed);
    assert!(!format!("{:?}", shown.lines()).contains("Stop"));
}

#[test]
fn the_state_words_are_plain() {
    let words: Vec<_> = [
        HookState::Installed,
        HookState::Missing,
        HookState::Swift,
        HookState::NotManaged,
        HookState::Unknown,
    ]
    .iter()
    .map(|s| s.word())
    .collect();
    assert_eq!(
        words,
        [
            Some("Installed"),
            Some("Not installed"),
            Some("Swift app"),
            Some("Not managed"),
            None
        ]
    );
}

#[test]
fn a_detached_daemon_says_so_above_the_five_unchecked_rows() {
    let shown = HooksShown::from_plan(Err(DETACHED.to_owned()));
    assert_eq!(shown, HooksShown::Detached);
    assert_eq!(shown.status().as_deref(), Some(DETACHED));
    assert_eq!(states(&shown), [HookState::Unknown; 5]);
}

#[test]
fn a_daemon_error_shows_its_message() {
    let shown = HooksShown::from_plan(Err("the daemon's plan isn't one this app reads".into()));
    assert_eq!(
        shown.status().as_deref(),
        Some("the daemon's plan isn't one this app reads")
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
        let status = shown.status().unwrap();
        assert_eq!(
            status,
            format!("Claude Code's settings.json was left alone: it {category}.")
        );
        assert!(!status.contains("SECRET-TOKEN"));
        assert_eq!(states(&shown), [HookState::Unknown; 5]);
    }
    let none = HooksShown::from_plan(Ok(facts("refused", None, vec![])));
    assert!(none.status().unwrap().ends_with("can't be read."));
}

#[test]
fn checking_is_one_plain_line() {
    assert_eq!(
        HooksShown::Checking.status().as_deref(),
        Some(t::settings::hooks_checking())
    );
}
