//! A switch never overwrites a login it hasn't saved: the main login, a lane's, or one too long for
//! Claude Code's own item.

use super::tests_support::*;
use super::*;
use crate::lanes::service_for;
use serde_json::Value;

/// A `claudeAiOauth` the size of one with many scopes and long tokens (filler, not tokens).
fn big_oauth(n: u8, v: u8) -> Value {
    let mut o = oauth(n, v, LATER);
    let filler: String = "FAKE-scope-".chars().cycle().take(5_000).collect();
    o["scopes"] = json!([filler]);
    o
}

#[test]
fn a_live_login_no_account_can_be_found_for_is_never_overwritten() {
    let w = World::new("safe-anon");
    // Expired, so the profile can't be asked; and ~/.claude.json names no account.
    let item = json!({"claudeAiOauth": oauth(1, 1, NOW - 1)}).to_string();
    w.chain.put(LIVE_SERVICE, "me", &item);
    w.tmp.write("home/.claude.json", "{}");
    w.saved(2, 1, LATER);

    let e = w.store.switch_to(&id(2)).unwrap_err();

    assert!(e.0.contains("couldn't be saved"), "{e}");
    assert_eq!(
        w.chain.get(LIVE_SERVICE, "me").as_deref(),
        Some(item.as_str())
    );
    assert_eq!(w.config_text(), "{}");
}

#[test]
fn a_long_live_login_is_saved_in_parts_and_switching_back_to_it_is_refused_cleanly() {
    let w = World::new("safe-long");
    w.signed_in(1, 1);
    let item = json!({"claudeAiOauth": big_oauth(1, 1)}).to_string();
    w.chain.put(LIVE_SERVICE, "me", &item);
    w.answers_profile(1);
    w.saved(2, 1, LATER);

    w.store.switch_to(&id(2)).unwrap();

    let saved = w
        .store
        .saved_login(&id(1))
        .unwrap()
        .and_then(|s| s.object("claudeAiOauth"))
        .map(Login::into_value);
    assert_eq!(
        saved,
        Some(big_oauth(1, 1)),
        "the long login was kept whole"
    );
    assert!(w
        .chain
        .get(SAVED_SERVICE, &format!("{}#0", id(1)))
        .is_some());
    // Claude Code's own item can't take it in one line: refused before anything changes.
    let (live, config) = (w.chain.get(LIVE_SERVICE, "me"), w.config_text());
    let e = w.store.switch_to(&id(1)).unwrap_err();
    assert!(e.0.contains("too long"), "{e}");
    assert_eq!(w.chain.get(LIVE_SERVICE, "me"), live);
    assert_eq!(w.config_text(), config);
}

/// Main on 1, a lane on 2, account 3 saved.
fn lane_world(tag: &str) -> (World, crate::lanes::Lane, String) {
    let w = World::new(tag);
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.saved(2, 1, LATER);
    w.saved(3, 1, LATER);
    w.tmp.write("home/.claude/settings.json", "{}");
    let lane = w.store.open_lane(&id(2)).unwrap();
    let service = service_for(&lane.path).unwrap();
    (w, lane, service)
}

#[test]
fn a_lane_login_that_is_another_accounts_is_never_removed_or_replaced() {
    let (w, lane, service) = lane_world("safe-lane-other");
    let other = json!({"claudeAiOauth": oauth(2, 2, LATER)}).to_string();
    w.chain.put(&service, "me", &other);
    w.answers_profile(3);

    let e = w.store.close_lane(&lane.name).unwrap_err();
    assert!(e.0.contains("couldn't be saved"), "{e}");
    assert_eq!(w.chain.get(&service, "me").as_deref(), Some(other.as_str()));
    assert_eq!(w.store.lanes().len(), 1);

    w.answers_profile(3);
    let e = w.store.switch_lane(&lane.name, &id(3)).unwrap_err();
    assert!(e.0.contains("couldn't be saved"), "{e}");
    assert_eq!(w.chain.get(&service, "me").as_deref(), Some(other.as_str()));
}

#[test]
fn a_lane_whose_account_has_no_saved_slot_keeps_its_login() {
    let (w, lane, service) = lane_world("safe-lane-noslot");
    let before = w.chain.get(&service, "me");
    w.chain.put(SAVED_SERVICE, &id(2), "");
    assert!(w.store.close_lane(&lane.name).is_err());
    assert_eq!(w.chain.get(&service, "me"), before);
}
