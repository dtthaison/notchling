use super::*;
use crate::skills_family::{KEY_RULE, REASON_RULE};

const BINARY: &str = "/Apps/N";

fn text() -> String {
    content(BINARY)
}

#[test]
fn accounts_skill_names_no_email() {
    let text = text();
    assert!(!text.contains('@'));
    assert!(!text.contains("email:"));
    assert!(text.contains("acct-"));
    assert!(text.contains("lane-"));
    assert!(text.contains("never by an email"));
}

#[test]
fn the_skill_reads_accounts_and_lanes_before_a_change() {
    let text = text();
    assert!(text.contains(&format!("'{BINARY}' accounts --json")));
    assert!(text.contains(&format!("'{BINARY}' lanes list --json")));
    assert!(text.contains("needsLogin"));
}

#[test]
fn limits_live_on_the_delegate_skill() {
    let text = text();
    assert!(text.contains("notchling-delegate"));
    assert!(text.contains("limits"));
    assert!(text.contains("wait for the reset"));
}

#[test]
fn every_change_has_a_dry_run_first() {
    let text = text();
    for act in [
        "accounts switch",
        "accounts add",
        "accounts remove",
        "accounts move-all",
        "accounts move",
        "lanes create",
        "lanes remove",
        "lanes switch",
    ] {
        assert!(
            text.lines()
                .any(|l| l.contains(&format!("'{BINARY}' {act} ")) && l.contains("--dry-run")),
            "{act}"
        );
    }
}

#[test]
fn a_switch_or_login_always_goes_through_a_card() {
    let text = text();
    assert!(text.contains("always asks the user on a card"));
    assert!(text.contains("Switching and logging in"));
    assert!(text.contains("--no-wait"));
    assert!(text.contains("consent <id> --wait"));
    assert!(text.contains("exit 5"));
    assert!(text.contains("exit 124"));
}

#[test]
fn the_skill_never_refreshes_a_token_or_asks_for_a_password() {
    let text = text();
    assert!(text.contains("Never refresh a token"));
    assert!(text.contains("Never ask for a password in the chat"));
    assert!(text.contains("~/.claude.json"));
    assert!(text.contains("Keychain"));
}

#[test]
fn the_skill_follows_a_refusal_once() {
    let text = text();
    assert!(text.contains("switch_off"));
    assert!(text.contains("error.ask"));
    assert!(text.contains("other_organization"));
}

#[test]
fn the_skill_carries_the_rules() {
    let text = text();
    assert!(text.contains(REASON_RULE));
    assert!(text.contains(KEY_RULE));
    assert!(text.contains("300 characters"));
}
