use crate::accounts_ops_tests::{profile, slot, world, World, ID1, ID2};
use crate::acting_ops_tests::app;
use crate::event_assert::{of, text};
use crate::limits_sources::ClaudeLimits;
use crate::op_group::OpGroup;
use notchling_engine::accounts::claude::{LIVE_SERVICE, SAVED_SERVICE};
use notchling_engine::accounts::fake::{MemoryKeychain, ScriptedHttp};
use notchling_engine::accounts::http::PROFILE_URL;
use notchling_engine::limits::sources::{ClaudeSource, LimitWhy};
use notchling_events::kinds::choices::LimitWindow;
use notchling_local_api::ops;
use serde_json::json;
use std::sync::Arc;

fn signed_in_as_one(w: &World, chain: &MemoryKeychain, http: &ScriptedHttp) {
    std::fs::write(
        w.d.0.join("home/.claude.json"),
        "{\"oauthAccount\": {\"accountUuid\": \"a1a1\", \"organizationUuid\": \"0101\", \"emailAddress\": \"one@example.com\"}}",
    )
    .unwrap();
    chain.put(
        LIVE_SERVICE,
        "me",
        r#"{"claudeAiOauth":{"accessToken":"test-access-1"}}"#,
    );
    chain.put(SAVED_SERVICE, ID2, &slot(ID2, "two@example.com", "2"));
    http.answer(PROFILE_URL, 200, &profile(ID1, "one@example.com"));
}

fn to(w: &World) -> Vec<String> {
    of(&w.sink, "account.switched")
        .iter()
        .filter_map(|e| text(e, "to"))
        .collect()
}

fn causes(w: &World) -> Vec<String> {
    of(&w.sink, "account.switched")
        .iter()
        .filter_map(|e| text(e, "cause"))
        .collect()
}

fn file(w: &World) -> std::path::PathBuf {
    w.d.0.join("data/account-log.json")
}

#[test]
fn the_switch_op_records_the_account_it_went_to() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world("log-op-switch", true, chain.clone(), http.clone());
    signed_in_as_one(&w, &chain, &http);
    w.ops
        .call(&app(), ops::ACCOUNTS_SWITCH, &json!({"id": ID2}))
        .unwrap();
    assert_eq!(to(&w), [ID1, ID2]);
    assert_eq!(causes(&w), ["other", "user"]);
    assert!(!file(&w).exists(), "account-log.json is not written");
}

#[test]
fn a_listing_that_syncs_records_a_login_changed_outside() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world("log-op-list", true, chain.clone(), http.clone());
    signed_in_as_one(&w, &chain, &http);
    w.ops
        .call(&app(), ops::ACCOUNTS_LIST, &json!({"sync": true}))
        .unwrap();
    assert_eq!(to(&w), [ID1]);
    assert_eq!(causes(&w), ["other"]);
}

#[test]
fn the_limits_auto_switch_records_its_window_and_percent() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world("log-limits", true, chain.clone(), http.clone());
    signed_in_as_one(&w, &chain, &http);
    let source = ClaudeLimits::new(w.flag.clone(), w.ops.store());
    source
        .switch_for_limit(
            ID2,
            &LimitWhy {
                window: Some(LimitWindow::Weekly),
                used_pct: Some(93.0),
            },
        )
        .unwrap();
    assert_eq!(to(&w), [ID1, ID2]);
    let last = of(&w.sink, "account.switched").pop().unwrap();
    assert_eq!(text(&last, "cause").as_deref(), Some("limit_auto"));
    assert_eq!(text(&last, "window").as_deref(), Some("weekly"));
    assert_eq!(text(&last, "used_pct").as_deref(), Some("93"));
    assert_eq!(text(&last, "from").as_deref(), Some(ID1));
}

#[test]
fn move_all_records_the_main_login_it_switched() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world("log-move-all", true, chain.clone(), http.clone());
    signed_in_as_one(&w, &chain, &http);
    let store = w.ops.store();
    let plan = notchling_engine::control::MoveAllPlan {
        switch_main: Some(ID2.to_string()),
        ..Default::default()
    };
    let done = store.move_all_accounts(&plan);
    assert_eq!(done.switched_to.as_deref(), Some(ID2));
    assert_eq!(to(&w), [ID1, ID2]);
    assert_eq!(causes(&w), ["other", "move_all"]);
}

#[test]
fn removing_an_account_records_account_login() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world("log-op-remove", true, chain.clone(), http.clone());
    signed_in_as_one(&w, &chain, &http);
    w.ops
        .call(&app(), ops::ACCOUNTS_REMOVE, &json!({"id": ID2}))
        .unwrap();
    let logins = of(&w.sink, "account.login");
    assert_eq!(logins.len(), 1);
    assert_eq!(text(&logins[0], "action").as_deref(), Some("removed"));
    assert_eq!(text(&logins[0], "account").as_deref(), Some(ID2));
}
