use super::follow::{AccountFollow, GRACE_MS, SIGN_IN_FOR_MS, SLOW_FOR_MS};
use super::rig::*;
use super::*;
use crate::consent_testkit::agent_caller;
use notchling_engine::providers::ProviderId;
use notchling_local_api::topics;

const ADDED: &str = "a9a9:0909";
const OTHER_ADDED: &str = "a0a0:1010";
const C3: &str = "c3c3-0003";
const C0: &str = "c0c0-0000";

fn ended(r: &Rig, login: &str, state: &str, account: Option<&str>) {
    if let (Some(raw), "added") = (account, state) {
        r.list_new(Kind::Claude, raw);
    }
    r.asks.follow().saw(
        topics::ACCOUNTS,
        topics::ACCOUNTS_ADD_ENDED,
        &json!({"login": login, "state": state,
            "account": account.map(|id| json!({"id": id, "email": "new@work.com"}))}),
    );
}

fn signing_in(tag: &str) -> (Rig, String) {
    let r = rig(tag);
    let id = r.card("add", json!({"tool": "claude"}));
    let reply = r
        .apply(
            &id,
            json!({"login": "L1", "url": "https://claude.ai/oauth?code=SECRET"}),
        )
        .unwrap();
    assert_eq!(reply["login"], "L1");
    assert_eq!(r.state(&id), "applying");
    assert_eq!(
        r.ran(),
        vec![(ops::ACCOUNTS_ADD_BEGIN.to_string(), json!({}))]
    );
    (r, id)
}

fn provider_changed(r: &Rig, provider: &str, reason: &str, state: &str) {
    r.asks.follow().saw(
        topics::PROVIDERS,
        topics::PROVIDERS_CHANGED,
        &json!({"provider": provider, "reason": reason, "state": state}),
    );
}

fn accounts_done(r: &Rig, ticket: u64, ok: bool, result: Value) {
    r.asks.follow().saw(
        topics::ACCOUNTS,
        topics::ACCOUNTS_DONE,
        &json!({"ticket": ticket, "ok": ok, "result": result}),
    );
}

#[test]
fn the_sign_in_s_three_endings() {
    let (r, id) = signing_in("added");
    ended(&r, "L0", "added", Some(OTHER_ADDED));
    assert_eq!(r.state(&id), "applying", "another sign-in's end");
    ended(&r, "L1", "added", Some(ADDED));
    assert_eq!(r.state(&id), "applied");
    assert_eq!(
        r.row(&id).result,
        Some(json!({"action": "add", "done": true, "tool": "claude",
            "account": r.acct(ADDED)}))
    );
    assert!(r.asks.follow().is_empty());

    for (n, state) in ["cancelled", "failed"].into_iter().enumerate() {
        let (r, id) = signing_in(&format!("ends-{n}"));
        r.advance(SIGN_IN_FOR_MS - 1);
        ended(&r, "L1", state, None);
        assert_eq!(r.state(&id), "failed", "{state}");
        assert!(r.asks.follow().is_empty());
    }

    let (r, id) = signing_in("silent");
    r.advance(SIGN_IN_FOR_MS + GRACE_MS - 1);
    r.consent.tick();
    assert_eq!(r.state(&id), "applying");
    r.advance(1);
    r.consent.tick();
    assert_eq!(
        r.state(&id),
        "withdrawn",
        "no end by the limit and its grace"
    );
    assert!(r.asks.follow().is_empty());

    let (r, id) = signing_in("late");
    r.advance(SIGN_IN_FOR_MS);
    ended(&r, "L1", "failed", None);
    assert_eq!(
        r.state(&id),
        "withdrawn",
        "a failure at the limit is the limit"
    );
}

#[test]
fn a_sign_in_whose_login_is_not_known_yet_takes_the_first_end() {
    let r = rig("first-end");
    let id = r.card("add", json!({"tool": "claude"}));
    let follow = r.asks.follow().clone();
    let reply = r
        .apply_with(&id, move |_| {
            follow.saw(
                topics::ACCOUNTS,
                topics::ACCOUNTS_ADD_ENDED,
                &json!({"login": "L7", "state": "added", "account": {"id": ADDED}}),
            );
            Ok(json!({"login": "L7"}))
        })
        .unwrap();
    assert_eq!(reply["login"], "L7");
    assert_eq!(r.state(&id), "applied");
}

#[test]
fn a_provider_add_ends_on_its_changed_event_with_the_new_account() {
    let r = rig("provider-add");
    let id = r.card("add", json!({"tool": "codex"}));
    r.apply(&id, json!({"state": "waiting", "attempt": 7}))
        .unwrap();
    assert_eq!(r.state(&id), "applying");
    r.add_changed("codex", 7, "started");
    r.add_changed("codex", 7, "waiting");
    r.add_changed("antigravity", 7, "done");
    r.add_changed("codex", 6, "done");
    r.add_changed("codex", 8, "done");
    provider_changed(&r, "codex", "switch", "done");
    assert_eq!(r.state(&id), "applying");
    r.add_ends(7, &[C3]);
    r.add_changed("codex", 7, "done");
    assert_eq!(r.state(&id), "applied");
    let new = r.book.ids.id(ProviderId::CODEX, C3).unwrap();
    assert_eq!(
        r.row(&id).result,
        Some(json!({"action": "add", "done": true, "tool": "codex", "account": new}))
    );
    let placed = r.book.with(|w| w.placed.clone());
    assert_eq!(placed, vec![(Kind::Codex, C3.to_string(), ORG.to_string())]);

    let id = r.card("add", json!({"tool": "agy"}));
    r.apply(&id, json!({"state": "starting", "attempt": 8}))
        .unwrap();
    r.add_changed("antigravity", 8, "cancelled");
    assert_eq!(r.state(&id), "failed");

    let id = r.card("add", json!({"tool": "codex"}));
    r.apply(&id, json!({"state": "starting", "attempt": 9}))
        .unwrap();
    r.advance(900_000 + GRACE_MS);
    r.consent.tick();
    assert_eq!(
        r.state(&id),
        "withdrawn",
        "none by the add limit and its grace"
    );
}

#[test]
fn a_slow_op_stays_applying_until_its_ticket_is_done() {
    let r = rig("ticket");
    let id = r.card("move_all", json!({"to": "main"}));
    let reply = r
        .apply(&id, json!({"state": "working", "ticket": 7}))
        .unwrap();
    assert_eq!(reply["ticket"], 7);
    assert_eq!(r.state(&id), "applying");
    accounts_done(&r, 8, true, json!({"moved": ["agent-7"], "errors": []}));
    assert_eq!(r.state(&id), "applying");
    r.advance(SLOW_FOR_MS - 1);
    r.consent.tick();
    assert_eq!(r.state(&id), "applying");
    accounts_done(&r, 7, true, json!({"moved": ["agent-7"], "errors": []}));
    assert_eq!(r.state(&id), "applied");
    assert_eq!(
        r.row(&id).result,
        Some(json!({"action": "move_all", "done": true, "moved": 1, "errors": 0}))
    );

    let r = rig("ticket-fails");
    let id = r.card("lane_create", json!({"account": r.acct(A2)}));
    r.apply(&id, json!({"state": "working", "ticket": 3}))
        .unwrap();
    accounts_done(&r, 3, false, Value::Null);
    assert_eq!(r.state(&id), "failed");

    let r = rig("ticket-silent");
    let id = r.card("lane_create", json!({"account": r.acct(A2)}));
    r.apply(&id, json!({"state": "working", "ticket": 4}))
        .unwrap();
    r.advance(SLOW_FOR_MS);
    r.consent.tick();
    assert_eq!(r.state(&id), "withdrawn");
}

#[test]
fn a_provider_switch_and_remove_end_on_their_changed_events() {
    let r = rig("provider-switch");
    let id = r.card("switch", json!({"account": r.codex("c2c2-0002")}));
    r.apply(&id, json!({"state": "switching"})).unwrap();
    assert_eq!(r.state(&id), "applying");
    provider_changed(&r, "codex", "switch", "done");
    assert_eq!(r.state(&id), "applied");

    let id = r.card("remove", json!({"account": r.codex("c2c2-0002")}));
    r.apply(&id, json!({"removed": false})).unwrap();
    assert_eq!(r.state(&id), "applying");
    provider_changed(&r, "codex", "remove", "done");
    assert_eq!(r.state(&id), "applied");
}

#[test]
fn a_ticket_that_ends_before_its_watch_is_registered_still_settles() {
    let r = rig("race");
    let id = r.card("move_all", json!({"to": "main"}));
    let follow = r.asks.follow().clone();
    r.apply_with(&id, move |_| {
        follow.saw(
            topics::ACCOUNTS,
            topics::ACCOUNTS_DONE,
            &json!({"ticket": 9, "ok": true, "result": {"moved": ["agent-7"], "errors": []}}),
        );
        Ok(json!({"state": "working", "ticket": 9}))
    })
    .unwrap();
    assert_eq!(r.state(&id), "applied");
    assert!(r.asks.follow().is_empty());
    assert_eq!(r.asks.follow().holding(), 0);

    let id = r.card("switch", json!({"account": r.codex("c2c2-0002")}));
    let follow = r.asks.follow().clone();
    r.apply_with(&id, move |_| {
        follow.saw(
            topics::PROVIDERS,
            topics::PROVIDERS_CHANGED,
            &json!({"provider": "codex", "reason": "switch", "state": "done"}),
        );
        Ok(json!({"state": "switching"}))
    })
    .unwrap();
    assert_eq!(r.state(&id), "applied");
    assert_eq!(r.asks.follow().holding(), 0);
}

#[test]
fn a_run_that_fails_leaves_nothing_followed() {
    let r = rig("run-fails");
    let id = r.card("move_all", json!({"to": "main"}));
    let refused = r
        .apply_with(&id, |_| Err(ApiError::bad_request("no")))
        .unwrap_err();
    assert_eq!(refused.message, "no");
    assert_eq!(r.state(&id), "failed");
    assert!(r.asks.follow().is_empty());
    assert_eq!(r.asks.follow().holding(), 0);
}

#[test]
fn the_sign_in_url_reaches_only_the_app() {
    let (r, id) = signing_in("url");
    let codex = r.card("add", json!({"tool": "codex"}));
    let reply = r
        .apply(
            &codex,
            json!({"state": "waiting", "attempt": 3,
                "url": "https://auth.openai.com/device?code=SECRET"}),
        )
        .unwrap();
    assert!(reply["url"].as_str().unwrap().starts_with("https://"));
    ended(&r, "L1", "added", Some(ADDED));
    assert_eq!(r.state(&id), "applied");
    let got = r
        .consent
        .call(&agent_caller(), ops::CONSENT_GET, &json!({"id": id}))
        .unwrap();
    let all = format!("{got}|{}", r.everywhere(&[id, codex]));
    for bad in ["https://", "code=", "SECRET", "oauth", ADDED, "new@"] {
        assert!(!all.contains(bad), "{bad} leaked");
    }
}

#[test]
fn a_run_that_fails_keeps_its_message_from_the_row_and_the_agent() {
    let r = rig("run-fails-quiet");
    let id = r.card("switch", json!({"account": r.acct(A2)}));
    let refused = r
        .apply_with(&id, |_| {
            Err(ApiError::bad_request(
                "ann@work.com a1a1:0101 /home/u/.claude https://claude.ai/x",
            ))
        })
        .unwrap_err();
    assert!(refused.message.contains("ann@work.com"));
    assert_eq!(r.state(&id), "failed");
    let got = r
        .consent
        .call(&agent_caller(), ops::CONSENT_GET, &json!({"id": id}))
        .unwrap();
    let all = format!("{got}|{}", r.everywhere(&[id]));
    for bad in ["ann@", "a1a1", "/home/u", "https://"] {
        assert!(!all.contains(bad), "{bad} leaked in {all}");
    }
}

#[test]
fn another_sign_in_that_ends_during_the_run_is_not_this_one() {
    let r = rig("l0-ends");
    let id = r.card("add", json!({"tool": "claude"}));
    let follow = r.asks.follow().clone();
    let during = r.asks.clone();
    r.apply_with(&id, move |_| {
        assert!(during.follow().adding("claude"));
        follow.saw(
            topics::ACCOUNTS,
            topics::ACCOUNTS_ADD_ENDED,
            &json!({"login": "L0", "state": "cancelled"}),
        );
        Ok(json!({"login": "L1"}))
    })
    .unwrap();
    assert_eq!(r.state(&id), "applying");
    assert_eq!(r.asks.follow().holding(), 0);
    ended(&r, "L1", "added", Some(ADDED));
    assert_eq!(r.state(&id), "applied");
    assert_eq!(r.row(&id).result.unwrap()["account"], json!(r.acct(ADDED)));
}

fn changed_in(follow: &AccountFollow, attempt: u64, state: &str) {
    follow.saw(
        topics::PROVIDERS,
        topics::PROVIDERS_CHANGED,
        &json!({"provider": "codex", "reason": "add", "state": state, "attempt": attempt}),
    );
}

fn placed(r: &Rig) -> Vec<(Kind, String, String)> {
    r.book.with(|w| w.placed.clone())
}

fn settled(r: &Rig) {
    assert!(r.asks.follow().is_empty());
    assert_eq!(r.asks.follow().holding(), 0);
}

#[test]
fn a_provider_add_that_ends_during_the_run_is_not_taken_by_the_next_add() {
    for (n, end) in ["cancelled", "failed"].into_iter().enumerate() {
        let r = rig(&format!("codex-own-end-{n}"));
        let id = r.card("add", json!({"tool": "codex"}));
        let follow = r.asks.follow().clone();
        r.apply_with(&id, |_| {
            assert!(r.asks.follow().adding("codex"));
            changed_in(&follow, 5, "started");
            changed_in(&follow, 5, end);
            changed_in(&follow, 6, "started");
            Ok(json!({"state": "waiting", "attempt": 5}))
        })
        .unwrap();
        assert_eq!(r.state(&id), "failed", "{end}: its own end");
        settled(&r);
        r.add_ends(6, &[C0]);
        r.add_changed("codex", 6, "done");
        assert_eq!(r.state(&id), "failed");
        assert!(placed(&r).is_empty(), "the next add's account stays");
    }

    let r = rig("codex-own-done");
    let id = r.card("add", json!({"tool": "codex"}));
    let follow = r.asks.follow().clone();
    r.apply_with(&id, |_| {
        changed_in(&follow, 5, "started");
        r.add_ends(5, &[C3]);
        changed_in(&follow, 5, "done");
        changed_in(&follow, 6, "started");
        r.add_ends(6, &[C0]);
        changed_in(&follow, 6, "done");
        Ok(json!({"state": "waiting", "attempt": 5}))
    })
    .unwrap();
    assert_eq!(r.state(&id), "applied");
    settled(&r);
    assert_eq!(r.row(&id).result.unwrap()["account"], json!(r.codex(C3)));
    assert_eq!(
        placed(&r),
        vec![(Kind::Codex, C3.to_string(), ORG.to_string())]
    );
}

#[test]
fn another_provider_add_that_starts_and_ends_during_the_run_is_not_this_one() {
    let r = rig("codex-other-cancelled");
    let id = r.card("add", json!({"tool": "codex"}));
    let follow = r.asks.follow().clone();
    r.apply_with(&id, |_| {
        changed_in(&follow, 4, "started");
        changed_in(&follow, 4, "cancelled");
        Ok(json!({"state": "starting", "attempt": 5}))
    })
    .unwrap();
    assert_eq!(r.state(&id), "applying", "another add's end");
    assert_eq!(r.asks.follow().holding(), 0);
    r.add_changed("codex", 5, "started");
    r.add_ends(5, &[C3]);
    r.add_changed("codex", 5, "done");
    assert_eq!(r.state(&id), "applied");
    assert_eq!(r.row(&id).result.unwrap()["account"], json!(r.codex(C3)));
    assert_eq!(
        placed(&r),
        vec![(Kind::Codex, C3.to_string(), ORG.to_string())]
    );

    let r = rig("codex-other-done");
    let id = r.card("add", json!({"tool": "codex"}));
    let follow = r.asks.follow().clone();
    r.apply_with(&id, |_| {
        changed_in(&follow, 4, "started");
        r.add_ends(4, &[C0]);
        changed_in(&follow, 4, "done");
        Ok(json!({"state": "starting", "attempt": 5}))
    })
    .unwrap();
    assert_eq!(r.state(&id), "applying", "another add's end");
    assert!(placed(&r).is_empty());
    r.add_changed("codex", 5, "started");
    r.add_changed("codex", 4, "done");
    r.add_changed("antigravity", 5, "done");
    assert_eq!(r.state(&id), "applying");
    r.add_changed("codex", 5, "cancelled");
    assert_eq!(r.state(&id), "failed");
    assert!(placed(&r).is_empty());

    let r = rig("codex-other-done-run-fails");
    let id = r.card("add", json!({"tool": "codex"}));
    let follow = r.asks.follow().clone();
    let refused = r
        .apply_with(&id, |_| {
            changed_in(&follow, 4, "done");
            Err(ApiError::bad_request("busy"))
        })
        .unwrap_err();
    assert_eq!(refused.message, "busy");
    assert_eq!(r.state(&id), "failed");
    settled(&r);
}

#[test]
fn a_provider_add_that_fails_with_no_start_event_still_settles() {
    let r = rig("codex-no-start");
    let id = r.card("add", json!({"tool": "codex"}));
    let follow = r.asks.follow().clone();
    r.apply_with(&id, |_| {
        changed_in(&follow, 5, "waiting");
        changed_in(&follow, 5, "failed");
        Ok(json!({"state": "starting", "attempt": 5}))
    })
    .unwrap();
    assert_eq!(r.state(&id), "failed");
    settled(&r);
}

fn after_another_add_stored_an_account(tag: &str) -> (Rig, String) {
    let r = rig(tag);
    let id = r.card("add", json!({"tool": "codex"}));
    let follow = r.asks.follow().clone();
    r.apply_with(&id, |_| {
        r.add_ends(4, &[C0]);
        changed_in(&follow, 4, "done");
        changed_in(&follow, 5, "started");
        Ok(json!({"state": "waiting", "attempt": 5}))
    })
    .unwrap();
    assert_eq!(r.state(&id), "applying");
    (r, id)
}

#[test]
fn an_account_another_add_stored_before_this_one_began_is_not_this_one_s() {
    let (r, id) = after_another_add_stored_an_account("codex-ui-done-known");
    r.add_ends(5, &[]);
    r.add_changed("codex", 5, "done");
    assert_eq!(r.state(&id), "applied");
    assert_eq!(r.row(&id).result.unwrap()["account"], Value::Null);
    assert!(placed(&r).is_empty(), "the other add's account stays");

    let (r, id) = after_another_add_stored_an_account("codex-ui-done-new");
    r.add_ends(5, &[C3]);
    r.add_changed("codex", 5, "done");
    assert_eq!(r.state(&id), "applied");
    assert_eq!(r.row(&id).result.unwrap()["account"], json!(r.codex(C3)));
    assert_eq!(
        placed(&r),
        vec![(Kind::Codex, C3.to_string(), ORG.to_string())]
    );
}

#[test]
fn a_provider_add_names_an_account_only_when_its_attempt_stored_exactly_one() {
    let r = rig("codex-several");
    let id = r.card("add", json!({"tool": "codex"}));
    r.apply(&id, json!({"state": "waiting", "attempt": 5}))
        .unwrap();
    r.add_ends(5, &[C0, C3]);
    r.add_changed("codex", 5, "done");
    assert_eq!(r.state(&id), "applied");
    assert_eq!(r.row(&id).result.unwrap()["account"], Value::Null);
    assert!(placed(&r).is_empty());

    let r = rig("codex-no-record");
    let id = r.card("add", json!({"tool": "codex"}));
    r.apply(&id, json!({"state": "waiting", "attempt": 5}))
        .unwrap();
    r.add_ends(6, &[C3]);
    r.add_changed("codex", 5, "done");
    assert_eq!(r.state(&id), "applied");
    assert_eq!(r.row(&id).result.unwrap()["account"], Value::Null);
    assert!(placed(&r).is_empty());
}

#[test]
fn a_provider_add_whose_reply_names_no_attempt_is_not_followed() {
    let r = rig("codex-no-attempt");
    let id = r.card("add", json!({"tool": "codex"}));
    let follow = r.asks.follow().clone();
    r.apply_with(&id, |_| {
        changed_in(&follow, 5, "started");
        Ok(json!({"state": "waiting"}))
    })
    .unwrap();
    assert_eq!(r.state(&id), "failed");
    settled(&r);
    r.add_ends(5, &[C3]);
    r.add_changed("codex", 5, "done");
    assert!(placed(&r).is_empty());

    let id = r.card("add", json!({"tool": "codex"}));
    r.apply(&id, json!({"state": "waiting", "attempt": 6}))
        .unwrap();
    provider_changed(&r, "codex", "add", "done");
    assert_eq!(r.state(&id), "applying", "an end that names no attempt");
    r.add_changed("codex", 6, "cancelled");
    assert_eq!(r.state(&id), "failed");
}

#[test]
fn an_account_another_sign_in_added_first_is_not_moved() {
    let r = rig("l0-added-during-run");
    let id = r.card("add", json!({"tool": "claude"}));
    let follow = r.asks.follow().clone();
    let during = r.book.clone();
    r.apply_with(&id, move |_| {
        during.with(|w| w.listing[0].1.push((ADDED.to_string(), None)));
        follow.saw(
            topics::ACCOUNTS,
            topics::ACCOUNTS_ADD_ENDED,
            &json!({"login": "L0", "state": "added", "account": {"id": ADDED}}),
        );
        Ok(json!({"login": "L1"}))
    })
    .unwrap();
    assert_eq!(r.state(&id), "applying");
    ended(&r, "L1", "added", Some(ADDED));
    assert_eq!(r.state(&id), "applied");
    assert_eq!(r.row(&id).result.unwrap()["account"], Value::Null);
    assert!(r.book.with(|w| w.placed.is_empty()));

    let (r, id) = signing_in("l0-added-while-watching");
    ended(&r, "L0", "added", Some(ADDED));
    assert_eq!(r.state(&id), "applying");
    ended(&r, "L1", "added", Some(ADDED));
    assert_eq!(r.state(&id), "applied");
    assert_eq!(r.row(&id).result.unwrap()["account"], Value::Null);
    assert!(r.book.with(|w| w.placed.is_empty()));
}
