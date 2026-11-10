use super::tests_support::*;
use super::*;
use crate::accounts::http::TOKEN_URL;
use serde_json::Value;

#[test]
fn a_sync_saves_the_live_login_and_asks_the_profile_once_per_token() {
    let w = World::new("acc-sync");
    w.signed_in(1, 1);
    w.answers_profile(1);
    let snap = w.store.sync().unwrap();
    assert_eq!(snap.active, Some(id(1)));
    assert_eq!(snap.accounts[0].email, email(1));
    assert_eq!(snap.accounts[0].plan, "Max 5x");
    let slot = w.saved_item(1);
    assert_eq!(slot["claudeAiOauth"]["accessToken"], "test-access-1-1");
    assert_eq!(slot["oauthAccount"]["emailAddress"], email(1));
    // The same token again: no second profile request.
    w.store.sync().unwrap();
    assert_eq!(w.http.seen().len(), 1);
    assert_eq!(
        w.store.saved_list(),
        snap.accounts,
        "the list is in its document"
    );
    assert!(
        !w.tmp.path("data/daemon/accounts.json").exists(),
        "no file is written any more"
    );
}

#[test]
fn the_old_list_file_is_read_then_moved_into_the_database_by_the_first_save() {
    let w = World::new("acc-import");
    let old = w.tmp.write(
        "data/daemon/accounts.json",
        &serde_json::to_string(&[ClaudeAccount {
            id: id(5),
            email: email(5),
            plan: "Pro".into(),
            needs_login: false,
        }])
        .unwrap(),
    );
    assert_eq!(w.store.saved_list()[0].id, id(5));
    assert!(old.exists(), "a read moves nothing");
    assert!(
        !crate::store::exists(&w.tmp.path("data")),
        "and makes no database"
    );
    w.signed_in(1, 1);
    w.answers_profile(1);
    let snap = w.store.sync().unwrap();
    let ids: Vec<_> = snap.accounts.iter().map(|a| a.id.clone()).collect();
    assert_eq!(ids, [id(5), id(1)], "the old list's account is kept");
    assert!(!old.exists(), "moved in");
    assert_eq!(w.store.saved_list(), snap.accounts);
}

#[test]
fn two_stores_saving_at_once_leave_one_readable_list() {
    let w = World::new("acc-two-writers");
    let list = |n: u8| {
        vec![ClaudeAccount {
            id: id(n),
            email: email(n),
            plan: "Pro".into(),
            needs_login: false,
        }]
    };
    std::thread::scope(|scope| {
        for t in 1..=4u8 {
            let store = &w.store;
            let list = list(t);
            scope.spawn(move || {
                for _ in 0..10 {
                    store.save_list(&list).unwrap();
                }
            });
        }
    });
    let saved = w.store.saved_list();
    assert_eq!(saved.len(), 1, "one writer's whole list, never a mix");
    assert!((1..=4).any(|n| saved[0].id == id(n)));
}

#[test]
fn an_expired_live_login_is_matched_by_the_config_never_checked_or_renewed() {
    let w = World::new("acc-expired");
    w.signed_in(1, 1);
    let item = json!({"claudeAiOauth": oauth(1, 1, NOW - 1)});
    w.chain.put(LIVE_SERVICE, "me", &item.to_string());
    let snap = w.store.sync().unwrap();
    assert_eq!(snap.active, Some(id(1)));
    assert!(w.http.seen().is_empty(), "no profile check, no renewal");
}

#[test]
fn signed_out_means_no_active_account() {
    let w = World::new("acc-none");
    assert_eq!(w.store.sync().unwrap().active, None);
    assert!(w.http.seen().is_empty());
}

#[test]
fn a_switch_replaces_only_the_claude_login_and_the_oauth_account() {
    let w = World::new("acc-switch");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.saved(2, 1, LATER);
    let before_item = w.chain.get(LIVE_SERVICE, "me").unwrap();
    let before_config = w.config_text();
    let snap = w.store.switch_to(&id(2)).unwrap();
    assert_eq!(snap.active, Some(id(2)));
    let after_item = w.chain.get(LIVE_SERVICE, "me").unwrap();
    // The MCP login keeps its bytes; only claudeAiOauth's value changed.
    assert!(
        after_item.ends_with(r#","mcpOAuth":{"srv":{"accessToken":"test-mcp"}}}"#),
        "{after_item}"
    );
    assert_eq!(
        w.live_item()["claudeAiOauth"]["accessToken"],
        "test-access-2-1"
    );
    assert_ne!(before_item, after_item);
    let config = w.config_text();
    assert!(
        config.starts_with("{\n  \"numStartups\": 4,\n  \"oauthAccount\": {\n"),
        "{config}"
    );
    assert!(
        config.ends_with("  },\n  \"projects\": {\"/w\": {\"cost\": 1.50}}\n}\n"),
        "{config}"
    );
    assert!(config.contains(&email(2)) && !config.contains(&email(1)));
    let backup = std::fs::read_to_string(w.tmp.path("home/.claude.json.notchling-backup")).unwrap();
    assert_eq!(backup, before_config);
    // The login it left was saved first.
    assert_eq!(
        w.saved_item(1)["claudeAiOauth"]["accessToken"],
        "test-access-1-1"
    );
}

#[test]
fn a_switch_to_an_account_in_a_lane_or_without_a_login_changes_nothing() {
    let w = World::new("acc-switch-refused");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.saved(2, 1, LATER);
    let lanes = &w.store.env().lanes;
    let lane = crate::lanes::Lane {
        name: "two".into(),
        path: lanes.path_for("two"),
        account_id: id(2),
        email: email(2),
        folder: None,
    };
    lanes.save(&[lane]).unwrap();
    let item = w.chain.get(LIVE_SERVICE, "me");
    let config = w.config_text();
    let e = w.store.switch_to(&id(2)).unwrap_err();
    assert_eq!(
        e.0,
        format!(
            "{} is in use in the two lane. Switch or close that lane first.",
            email(2)
        )
    );
    let e = w.store.switch_to(&id(3)).unwrap_err();
    assert!(e.0.starts_with("No saved login for this account"), "{e}");
    assert_eq!(w.chain.get(LIVE_SERVICE, "me"), item);
    assert_eq!(w.config_text(), config);
}

#[test]
fn a_config_that_cant_take_the_switch_stops_it_before_the_keychain_changes() {
    let w = World::new("acc-switch-config");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.saved(2, 1, LATER);
    w.store.sync().unwrap();
    w.tmp.write("home/.claude.json", "{broken");
    let item = w.chain.get(LIVE_SERVICE, "me");
    assert!(w.store.switch_to(&id(2)).is_err());
    assert_eq!(w.chain.get(LIVE_SERVICE, "me"), item);
    assert_eq!(w.config_text(), "{broken");
}

#[test]
fn ids_that_arent_account_ids_never_reach_the_keychain() {
    let w = World::new("acc-bad-id");
    for bad in ["", "x", "a\"b:c", "a:b\nc"] {
        assert!(w.store.switch_to(bad).is_err());
        assert!(w.store.remove(bad).is_err());
        assert!(w.store.open_lane(bad).is_err());
    }
    assert!(w.chain.calls().is_empty());
}

#[test]
fn adding_saves_the_login_and_leaves_claude_code_alone() {
    let w = World::new("acc-add");
    w.signed_in(1, 1);
    w.answers_profile(3);
    let Value::Object(map) = oauth(3, 1, LATER) else {
        unreachable!()
    };
    let account = w.store.add(&Login(map)).unwrap();
    assert_eq!(account.id, id(3));
    assert_eq!(account.plan, "Max 5x");
    let slot = w.saved_item(3);
    assert_eq!(slot["claudeAiOauth"]["subscriptionType"], "max");
    assert_eq!(
        slot["claudeAiOauth"]["rateLimitTier"],
        "default_claude_max_5x"
    );
    assert_eq!(slot["oauthAccount"]["emailAddress"], email(3));
    assert_eq!(
        w.live_item()["claudeAiOauth"]["accessToken"],
        "test-access-1-1"
    );
    assert_eq!(
        w.http.seen()[0].1.as_deref(),
        Some("test-access-3-1"),
        "checked with its own token"
    );
    // A profile that doesn't answer: refused, and the message names no token.
    let Value::Object(map) = oauth(4, 1, LATER) else {
        unreachable!()
    };
    let e = w.store.add(&Login(map)).unwrap_err();
    assert_eq!(
        e.0,
        "Signed in, but couldn't read the account's profile. Try again."
    );
}

#[test]
fn removing_forgets_the_login_unless_a_lane_holds_it() {
    let w = World::new("acc-remove");
    w.saved(2, 1, LATER);
    w.saved(3, 1, LATER);
    let lanes = &w.store.env().lanes;
    lanes
        .save(&[crate::lanes::Lane {
            name: "three".into(),
            path: lanes.path_for("three"),
            account_id: id(3),
            email: email(3),
            folder: None,
        }])
        .unwrap();
    let snap = w.store.remove(&id(2)).unwrap();
    assert!(w.chain.get(SAVED_SERVICE, &id(2)).is_none());
    assert_eq!(snap.accounts.len(), 1);
    assert!(w.store.remove(&id(3)).unwrap_err().0.contains("three lane"));
    assert!(w.chain.get(SAVED_SERVICE, &id(3)).is_some());
}

#[test]
fn a_removal_stays_removed_after_a_later_sync() {
    // The Swift app's issue #7: one lock, so a sync can't write back a list read before a removal.
    let w = World::new("acc-remove-race");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.saved(2, 1, LATER);
    w.store.sync().unwrap();
    w.store.remove(&id(2)).unwrap();
    let snap = w.store.sync().unwrap();
    assert!(snap.accounts.iter().all(|a| a.id != id(2)));
}

#[test]
fn the_main_logins_token_is_never_renewed() {
    let w = World::new("acc-token-main");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.store.sync().unwrap();
    assert_eq!(
        w.store.usage_token(&id(1)).unwrap().unwrap().expose(),
        "test-access-1-1"
    );
    w.clock.store(LATER, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(w.store.usage_token(&id(1)).unwrap(), None);
    assert_eq!(w.token_requests(), 0);
}

#[test]
fn a_saved_login_live_by_the_config_file_is_never_renewed_either() {
    // No sync yet, so the store hasn't read the live login: ~/.claude.json still says it's main.
    let w = World::new("acc-token-config");
    w.signed_in(1, 1);
    w.saved(1, 1, NOW - 1);
    assert_eq!(w.store.usage_token(&id(1)).unwrap(), None);
    assert_eq!(w.token_requests(), 0);
}

#[test]
fn a_saved_login_live_nowhere_is_renewed_and_a_refusal_marks_it() {
    let w = World::new("acc-token-saved");
    w.saved(2, 1, NOW - 1);
    w.http.answer(
        TOKEN_URL,
        200,
        r#"{"access_token":"test-access-2-2","refresh_token":"test-refresh-2-2","expires_in":3600}"#,
    );
    assert_eq!(
        w.store.usage_token(&id(2)).unwrap().unwrap().expose(),
        "test-access-2-2"
    );
    let slot = w.saved_item(2);
    assert_eq!(slot["claudeAiOauth"]["refreshToken"], "test-refresh-2-2");
    assert_eq!(slot["claudeAiOauth"]["expiresAt"], NOW + 3_600_000);
    assert_eq!(
        slot["claudeAiOauth"]["scopes"],
        json!(["user:inference", "user:profile"])
    );
    // Expired again, and refused this time.
    w.clock
        .store(NOW + 10_000_000, std::sync::atomic::Ordering::SeqCst);
    w.http
        .answer(TOKEN_URL, 400, r#"{"error":"invalid_grant"}"#);
    assert_eq!(w.store.usage_token(&id(2)).unwrap(), None);
    assert!(w.store.saved_list()[0].needs_login);
    assert_eq!(w.saved_item(2), slot, "a refusal changes nothing");
}

#[test]
fn no_error_or_snapshot_carries_a_token() {
    let w = World::new("acc-no-tokens");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.saved(2, 1, LATER);
    let mut texts = vec![format!("{:?}", w.store.sync().unwrap())];
    texts.push(format!("{:?}", w.store.switch_to(&id(9))));
    texts.push(format!("{:?}", w.store.open_lane(&id(1))));
    texts.push(format!("{:?}", w.store.switch_lane("none", &id(2))));
    texts.push(
        crate::store::doc(&w.tmp.path("data"), super::LIST_DOC)
            .unwrap()
            .unwrap(),
    );
    for text in texts {
        assert!(
            !text.contains("test-access") && !text.contains("test-refresh"),
            "{text}"
        );
    }
}
