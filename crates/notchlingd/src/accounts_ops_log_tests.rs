use crate::accounts_ops_tests::{profile, slot, world, World, ID1, ID2};
use crate::acting_ops_tests::app;
use crate::limits_sources::ClaudeLimits;
use crate::op_group::OpGroup;
use notchling_engine::accounts::claude::{LIVE_SERVICE, SAVED_SERVICE};
use notchling_engine::accounts::fake::{MemoryKeychain, ScriptedHttp};
use notchling_engine::accounts::http::PROFILE_URL;
use notchling_engine::cost::account_log::{self, Entry};
use notchling_engine::limits::sources::ClaudeSource;
use notchling_local_api::ops;
use serde_json::json;
use std::sync::Arc;

const NOW_SECS: f64 = 1_800_000_000.0;

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

fn log(w: &World) -> Vec<Entry> {
    account_log::load(&w.d.0.join("data/account-log.json"))
}

fn ids(w: &World) -> Vec<String> {
    log(w).into_iter().map(|e| e.id).collect()
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
    assert_eq!(ids(&w), [ID1, ID2]);
    assert_eq!(
        account_log::account_at(NOW_SECS + 1.0, &log(&w)),
        ID2,
        "cost after the switch goes to the new account"
    );
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
    assert_eq!(ids(&w), [ID1]);
}

#[test]
fn the_limits_auto_switch_records_the_account_it_went_to() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world("log-limits", true, chain.clone(), http.clone());
    signed_in_as_one(&w, &chain, &http);
    let source = ClaudeLimits::new(w.flag.clone(), w.ops.store());
    source.switch_to(ID2).unwrap();
    assert_eq!(ids(&w), [ID1, ID2]);
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
    assert_eq!(ids(&w), [ID1, ID2]);
}
