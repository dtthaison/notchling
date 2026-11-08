use super::tests_support::*;
use super::*;
use crate::accounts::http::TOKEN_URL;
use crate::control::MoveAllPlan;
use crate::lanes::service_for;
use serde_json::Value;

fn lane_item(w: &World, lane: &crate::lanes::Lane) -> Value {
    let service = service_for(&lane.path).unwrap();
    serde_json::from_str(&w.chain.get(&service, "me").unwrap_or_default()).unwrap_or(Value::Null)
}

/// Main on account 1; accounts 2 and 3 saved.
fn world(tag: &str) -> World {
    let w = World::new(tag);
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.saved(2, 1, LATER);
    w.saved(3, 1, LATER);
    w.tmp.write("home/.claude/settings.json", "{}");
    w
}

#[test]
fn a_lane_gets_the_saved_login_in_its_own_item() {
    let w = world("lane-open");
    let lane = w.store.open_lane(&id(2)).unwrap();
    assert_eq!(lane.name, "user2");
    assert_eq!(
        lane.path,
        w.tmp.path("home/.claude-lanes/user2").to_string_lossy()
    );
    assert_eq!(
        lane_item(&w, &lane)["claudeAiOauth"]["accessToken"],
        "test-access-2-1"
    );
    let config =
        std::fs::read_to_string(std::path::Path::new(&lane.path).join(".claude.json")).unwrap();
    assert!(
        config.contains(&email(2)) && config.contains("\"cost\": 1.50"),
        "{config}"
    );
    assert_eq!(w.store.lanes(), vec![lane.clone()]);
    // Asking again returns the same lane.
    assert_eq!(w.store.open_lane(&id(2)).unwrap(), lane);
    // The main login's account can't have a lane.
    let e = w.store.open_lane(&id(1)).unwrap_err();
    assert_eq!(
        e.0,
        "That account is the main login. Open Claude Code as usual for it."
    );
    assert_eq!(w.store.lanes().len(), 1);
}

#[test]
fn an_expired_saved_login_is_renewed_before_a_lane_gets_it_and_a_refused_one_stops_it() {
    let w = world("lane-renew");
    w.saved(2, 1, NOW - 1);
    w.http.answer(TOKEN_URL, 200, r#"{"access_token":"test-access-2-2","refresh_token":"test-refresh-2-2","expires_in":3600}"#);
    let lane = w.store.open_lane(&id(2)).unwrap();
    assert_eq!(
        lane_item(&w, &lane)["claudeAiOauth"]["accessToken"],
        "test-access-2-2"
    );
    w.saved(3, 1, NOW - 1);
    w.http.answer(TOKEN_URL, 401, "{}");
    let e = w.store.open_lane(&id(3)).unwrap_err();
    assert!(e.0.contains("signed out"), "{e}");
    assert_eq!(w.store.lanes().len(), 1);
}

#[test]
fn switching_a_lane_gives_the_renewed_login_back_first() {
    let w = world("lane-switch");
    let lane = w.store.open_lane(&id(2)).unwrap();
    // Claude Code in the lane renewed the login there.
    let service = service_for(&lane.path).unwrap();
    w.chain.put(
        &service,
        "me",
        &json!({"claudeAiOauth": oauth(2, 2, LATER), "mcpOAuth": {"m": 1}}).to_string(),
    );
    w.answers_profile(2);
    let switched = w.store.switch_lane(&lane.name, &id(3)).unwrap();
    assert_eq!(switched.account_id, id(3));
    assert_eq!(switched.email, email(3));
    assert_eq!(
        w.saved_item(2)["claudeAiOauth"]["refreshToken"],
        "test-refresh-2-2"
    );
    let item = lane_item(&w, &lane);
    assert_eq!(item["claudeAiOauth"]["accessToken"], "test-access-3-1");
    assert_eq!(
        item["mcpOAuth"],
        json!({"m": 1}),
        "the rest of the item stays"
    );
    let config =
        std::fs::read_to_string(std::path::Path::new(&lane.path).join(".claude.json")).unwrap();
    assert!(config.contains(&email(3)));
    assert_eq!(w.store.lanes()[0].account_id, id(3));
}

#[test]
fn a_lane_login_that_the_profile_says_is_another_account_never_goes_back() {
    let w = world("lane-return-other");
    let lane = w.store.open_lane(&id(2)).unwrap();
    let service = service_for(&lane.path).unwrap();
    w.chain.put(
        &service,
        "me",
        &json!({"claudeAiOauth": oauth(2, 2, LATER)}).to_string(),
    );
    w.answers_profile(3);
    assert!(w.store.close_lane(&lane.name).unwrap());
    assert_eq!(
        w.saved_item(2)["claudeAiOauth"]["refreshToken"],
        "test-refresh-2-1"
    );
}

#[test]
fn one_account_is_live_in_one_place() {
    let w = world("lane-one-place");
    let two = w.store.open_lane(&id(2)).unwrap();
    let three = w.store.open_lane(&id(3)).unwrap();
    for target in [id(1), id(3)] {
        let e = w.store.switch_lane(&two.name, &target).unwrap_err();
        assert_eq!(
            e.0,
            "That account is already in use in the main login or another lane."
        );
    }
    assert!(
        w.store.switch_to(&id(3)).is_err(),
        "the main login can't take a lane's account"
    );
    assert_eq!(
        lane_item(&w, &three)["claudeAiOauth"]["accessToken"],
        "test-access-3-1"
    );
}

#[test]
fn closing_a_lane_returns_its_login_and_removes_its_item_and_folder() {
    let w = world("lane-close");
    let lane = w.store.open_lane(&id(2)).unwrap();
    let service = service_for(&lane.path).unwrap();
    w.chain.put(
        &service,
        "me",
        &json!({"claudeAiOauth": oauth(2, 3, NOW - 1)}).to_string(),
    );
    assert!(w.store.close_lane(&lane.name).unwrap());
    // Expired, so it couldn't be checked: it still goes back, as the Swift app did.
    assert_eq!(
        w.saved_item(2)["claudeAiOauth"]["refreshToken"],
        "test-refresh-2-3"
    );
    assert!(w.chain.get(&service, "me").is_none());
    assert!(!std::path::Path::new(&lane.path).exists());
    assert!(w.store.lanes().is_empty());
    assert!(
        w.tmp.path("home/.claude/settings.json").exists(),
        "the shared file stays"
    );
    assert!(!w.store.close_lane(&lane.name).unwrap());
    assert_eq!(w.token_requests(), 0, "a lane's login is never renewed");
}

#[test]
fn move_all_closes_the_lane_then_switches_the_main_login() {
    let w = world("lane-move-all");
    let lane = w.store.open_lane(&id(2)).unwrap();
    let plan = MoveAllPlan::make(&[], &w.store.lanes(), Some(&id(1)), Some(&id(2)));
    assert_eq!(plan.close_lane.as_deref(), Some(lane.name.as_str()));
    let done = w.store.move_all_accounts(&plan);
    assert_eq!(done.errors, Vec::<String>::new());
    assert_eq!(done.closed_lane, Some(lane.name));
    assert_eq!(done.switched_to, Some(id(2)));
    assert_eq!(
        w.live_item()["claudeAiOauth"]["accessToken"],
        "test-access-2-1"
    );
    assert!(w.config_text().contains(&email(2)));
    assert!(w.store.lanes().is_empty());
}
