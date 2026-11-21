use serde_json::{json, Value};

use super::*;

const NOW: i64 = 1_794_096_000_000;

fn env() -> Env {
    Env {
        now_ms: NOW,
        vault_on: true,
        shown_ms: 5_000,
        node: None,
    }
}

fn setting() -> Value {
    json!({
        "id": "c1", "kind": "setting", "target": "notchAutoOpen", "state": "waiting",
        "change": {"key": "notchAutoOpen", "title": "Auto-Open", "before": false, "after": true, "text": false},
        "facts": [{"line": "consent-setting-change", "args": {"key": "notchAutoOpen", "title": "Auto-Open"}}],
        "tool": "claude", "verified": true, "session": "0b1c2d3e4f5a", "organization": "Personal",
        "folder": "/Users/maker/work/notchling",
        "expiresMs": NOW + 14 * 60_000 + 3_000, "appliedBy": "daemon", "answerOn": "mac_or_phone"
    })
}

fn secret() -> Value {
    json!({
        "id": "c2", "kind": "secret_request", "target": "stripe-key", "state": "waiting",
        "change": {"name": "stripe-key", "update": false},
        "facts": [{"line": "consent-secret-request", "args": {"name": "stripe-key", "update": false, "vaultOn": true}}],
        "tool": "codex", "verified": false, "session": "9f8e7d6c5b4a", "folder": "/Users/maker/work/shop",
        "reason": "Needed to run the billing tests",
        "expiresMs": NOW + 60_000, "appliedBy": "app", "answerOn": "mac"
    })
}

#[test]
fn a_setting_shows_the_daemons_facts_and_the_two_values() {
    let card = ConsentCard::build(&setting(), &env()).expect("card");
    assert_eq!(
        card.facts,
        vec!["Change Auto-Open", "Now: false", "Would become: true"]
    );
    assert!(card.says.is_none());
    assert!(card.value.is_none());
    assert!(!card.app_applied);
}

#[test]
fn a_text_setting_puts_the_new_value_in_the_agents_box() {
    let mut view = setting();
    view["change"] = json!({"key": "k", "title": "Prompt", "before": "a", "after": "run this\nthen that", "text": true});
    let card = ConsentCard::build(&view, &env()).expect("card");
    assert_eq!(card.facts, vec!["Change Auto-Open"]);
    let value = card.value.expect("value box");
    assert_eq!(value.text, "run this\nthen that");
    assert!(value.note.is_none());
}

#[test]
fn a_secret_request_names_the_secret_and_the_folder() {
    let card = ConsentCard::build(&secret(), &env()).expect("card");
    assert_eq!(card.facts, vec!["Add the secret stripe-key"]);
    let labels: Vec<&str> = card.details.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(labels, vec!["Asked by", "Folder", "Covers"]);
    assert_eq!(card.details[1].value, "/Users/maker/work/shop");
    assert!(card.app_applied);
}

#[test]
fn every_kind_shows_the_folder_from_the_row() {
    let card = ConsentCard::build(&setting(), &env()).expect("card");
    let labels: Vec<&str> = card.details.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(labels, vec!["Asked by", "Folder", "Organization", "Covers"]);
    assert_eq!(card.details[1].value, "/Users/maker/work/notchling");
    let mut none = setting();
    none.as_object_mut().unwrap().remove("folder");
    let card = ConsentCard::build(&none, &env()).expect("card");
    assert!(card.details.iter().all(|d| d.label != "Folder"));
    let mut inside = secret();
    inside["change"]["folder"] = json!("/somewhere/else");
    let card = ConsentCard::build(&inside, &env()).expect("card");
    assert_eq!(card.details[1].value, "/Users/maker/work/shop");
}

#[test]
fn an_update_reads_as_an_update() {
    let mut view = secret();
    view["facts"][0]["args"]["update"] = json!(true);
    let card = ConsentCard::build(&view, &env()).expect("card");
    assert_eq!(card.facts, vec!["Update the secret stripe-key"]);
}

#[test]
fn an_unknown_fact_line_shows_nothing() {
    let mut view = secret();
    view["facts"] = json!([{"line": "consent-from-the-future", "args": {}}]);
    let card = ConsentCard::build(&view, &env()).expect("card");
    assert!(card.facts.is_empty());
}

#[test]
fn the_agents_reason_is_marked_untrusted_and_nothing_else_is() {
    let card = ConsentCard::build(&secret(), &env()).expect("card");
    let says = card.says.expect("reason");
    assert_eq!(says.label, "The agent says");
    assert_eq!(says.text, "Needed to run the billing tests");
    assert!(says.note.is_some());
    let none = ConsentCard::build(&setting(), &env()).expect("card");
    assert!(none.says.is_none());
}

#[test]
fn who_follows_how_well_the_session_is_known() {
    let verified = ConsentCard::build(&setting(), &env()).expect("card");
    assert_eq!(
        verified.details[0].value,
        "Claude Code · verified session 0b1c2d3e"
    );
    let unverified = ConsentCard::build(&secret(), &env()).expect("card");
    assert_eq!(
        unverified.details[0].value,
        "Codex · session 9f8e7d6c, not verified"
    );
    let mut outside = secret();
    outside["session"] = Value::Null;
    outside["tool"] = json!("shell");
    let card = ConsentCard::build(&outside, &env()).expect("card");
    assert_eq!(card.details[0].value, "A program outside any agent session");
    outside["root"] = json!("Terminal");
    let card = ConsentCard::build(&outside, &env()).expect("card");
    assert_eq!(
        card.details[0].value,
        "A program outside any agent session · in Terminal"
    );
}

#[test]
fn where_lists_the_node_and_the_organization_when_known() {
    let mut e = env();
    e.node = Some("m1".into());
    let card = ConsentCard::build(&setting(), &e).expect("card");
    let labels: Vec<&str> = card.details.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["Asked by", "Machine", "Folder", "Organization", "Covers"]
    );
}

#[test]
fn the_expiry_counts_down_and_ends() {
    let card = ConsentCard::build(&setting(), &env()).expect("card");
    assert_eq!(card.expiry, "Ends in 14m 03s");
    assert!(!card.ended);
    let mut late = env();
    late.now_ms = NOW + 15 * 60_000;
    let card = ConsentCard::build(&setting(), &late).expect("card");
    assert!(card.ended);
    assert_eq!(card.expiry, "Ended. It can no longer be allowed.");
}

#[test]
fn allow_is_off_until_a_second_after_the_card_is_shown() {
    let mut e = env();
    e.shown_ms = 0;
    let card = ConsentCard::build(&setting(), &e).expect("card");
    assert_eq!(card.allow, Allow::Wait(1_000));
    e.shown_ms = 400;
    let card = ConsentCard::build(&setting(), &e).expect("card");
    assert_eq!(card.allow, Allow::Wait(600));
    e.shown_ms = 1_000;
    let card = ConsentCard::build(&setting(), &e).expect("card");
    assert_eq!(card.allow, Allow::Ready);
    assert!(card.allow.enabled());
}

#[test]
fn allow_is_off_for_a_vault_request_while_the_vault_is_off() {
    let mut e = env();
    e.vault_on = false;
    let card = ConsentCard::build(&secret(), &e).expect("card");
    assert_eq!(card.allow, Allow::Off);
    assert_eq!(
        card.blocked.as_deref(),
        Some("The Vault is off. Turn it on in Settings first, then allow this.")
    );
    let on = ConsentCard::build(&secret(), &env()).expect("card");
    assert_eq!(on.allow, Allow::Ready);
    assert!(on.blocked.is_none());
}

#[test]
fn a_setting_ignores_the_vault_switch() {
    let mut e = env();
    e.vault_on = false;
    let card = ConsentCard::build(&setting(), &e).expect("card");
    assert_eq!(card.allow, Allow::Ready);
    assert!(card.blocked.is_none());
}

#[test]
fn allow_is_off_after_the_end_and_once_closed() {
    let mut late = env();
    late.now_ms = NOW + 60 * 60_000;
    assert_eq!(
        ConsentCard::build(&setting(), &late).expect("card").allow,
        Allow::Off
    );
    let mut closed = setting();
    closed["state"] = json!("denied");
    assert_eq!(
        ConsentCard::build(&closed, &env()).expect("card").allow,
        Allow::Off
    );
    let mut opened = setting();
    opened["state"] = json!("opened");
    let card = ConsentCard::build(&opened, &env()).expect("card");
    assert_eq!(card.allow, Allow::Ready);
    assert!(card.answerable);
    assert!(
        !ConsentCard::build(&closed, &env())
            .expect("card")
            .answerable
    );
    assert!(
        !ConsentCard::build(&setting(), &late)
            .expect("card")
            .answerable
    );
}

#[test]
fn a_view_without_an_id_or_an_end_is_no_card() {
    assert!(ConsentCard::build(&json!({"kind": "setting"}), &env()).is_none());
    assert!(ConsentCard::build(&json!({"id": "c"}), &env()).is_none());
}

#[test]
fn the_vault_switch_or_its_entries_moving_refreshes_the_cards() {
    assert!(
        !vault_moved(None, (0, false)),
        "the first look moves nothing"
    );
    assert!(!vault_moved(Some((3, true)), (3, true)));
    assert!(
        vault_moved(Some((3, false)), (3, true)),
        "the switch turned on"
    );
    assert!(vault_moved(Some((3, true)), (4, true)), "the vault changed");
}

#[test]
fn the_card_blocks_a_secret_only_while_the_vault_is_off_and_follows_it_back_on() {
    let mut e = env();
    e.vault_on = false;
    let off = ConsentCard::build(&secret(), &e).expect("card");
    assert!(off.blocked.is_some() && !off.allow.enabled());
    e.vault_on = true;
    let on = ConsentCard::build(&secret(), &e).expect("card");
    assert!(on.blocked.is_none() && on.allow.enabled());
    e.vault_on = false;
    let setting = ConsentCard::build(&setting(), &e).expect("card");
    assert!(
        setting.blocked.is_none(),
        "a setting never waits on the Vault"
    );
}
