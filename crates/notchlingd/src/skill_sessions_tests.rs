use super::*;
use crate::skills_family::{KEY_RULE, REASON_RULE};

const BINARY: &str = "/Apps/N";

fn text() -> String {
    content(BINARY)
}

#[test]
fn the_skill_finds_a_session_two_ways() {
    let text = text();
    assert!(text.contains(&format!("'{BINARY}' sessions --json")));
    assert!(text.contains("--kind sessions --since"));
}

#[test]
fn the_skill_says_who_may_act_free() {
    let text = text();
    assert!(text.contains("self"));
    assert!(text.contains("session you started"));
    assert!(text.contains("Your own session"));
    assert!(text.contains("a card"));
}

#[test]
fn every_act_has_a_dry_run_first() {
    let text = text();
    for act in [
        "session new",
        "session reply",
        "session stop",
        "session remove",
        "session answer",
        "session command",
        "session dialog",
        "session chrome",
        "session options",
    ] {
        assert!(
            text.lines()
                .any(|l| l.contains(&format!("'{BINARY}' {act}")) && l.contains("--dry-run")),
            "{act}"
        );
    }
}

#[test]
fn the_skill_waits_in_the_background_and_follows_a_refusal_once() {
    let text = text();
    assert!(text.contains("--no-wait"));
    assert!(text.contains("watch --topic mine.consents --until applied"));
    assert!(text.contains("switch_off"));
    assert!(text.contains("error.ask"));
    assert!(text.contains("once"));
    assert!(text.contains("exit 6"));
    assert!(text.contains("exit 124"));
}

#[test]
fn the_skill_carries_the_rules() {
    let text = text();
    assert!(text.contains(REASON_RULE));
    assert!(text.contains(KEY_RULE));
    assert!(text.contains("300 characters"));
    assert!(text.contains("first line"));
}

#[test]
fn reason_and_key_rules_are_in_every_acting_skill() {
    let texts = [
        content(BINARY),
        crate::skill_jobs::content(BINARY),
        crate::skill_cluster::content(BINARY),
        crate::skill_machines::content(BINARY),
        crate::skill_accounts::content(BINARY),
    ];
    for text in texts {
        assert!(text.contains(REASON_RULE), "{}", &text[..80]);
        assert!(text.contains(KEY_RULE), "{}", &text[..80]);
    }
}
