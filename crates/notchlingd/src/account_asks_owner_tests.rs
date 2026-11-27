use super::rig::*;
use super::*;
use notchling_engine::organizations::PERSONAL_ID;
use notchling_engine::providers::ProviderAccount;
use notchling_local_api::topics;

const A8: &str = "a8a8:0808";
const A9: &str = "a9a9:0909";
const C3: &str = "c3c3-0003";

fn refused(r: &Rig, action: &str, args: Value) -> String {
    let err = r.ask(action, args.clone()).unwrap_err();
    reason(&err)
}

#[test]
fn a_move_to_main_checks_the_main_login_it_lands_on() {
    let r = rig("main-owner");
    r.book.with(|w| w.snap.active = Some(A4.to_string()));
    assert_eq!(refused(&r, "move_all", json!({"to": "main"})), "never");
    for raw in [A5, A6] {
        r.book.with(|w| w.snap.active = Some(raw.to_string()));
        assert_eq!(
            refused(&r, "move_all", json!({"to": "main"})),
            "unknown_owner",
            "{raw}"
        );
    }
    r.book.with(|w| {
        w.snap.active = Some(A1.to_string());
        w.agents.push(agent("proc-9", false, MAIN, THERE));
    });
    assert_eq!(refused(&r, "move_all", json!({"to": "main"})), "never");
    r.book.with(|w| w.agents.retain(|a| a.id != "proc-9"));
    r.book.with(|w| w.snap.active = None);
    assert_eq!(refused(&r, "move_all", json!({"to": "main"})), "signed_out");
    r.book.with(|w| {
        w.snap.active = Some(A1.to_string());
        w.snap.accounts[0].needs_login = true;
    });
    assert_eq!(refused(&r, "move_all", json!({"to": "main"})), "signed_out");
    assert_eq!(r.open_count(), 0);
    assert!(r.ran().is_empty());
    r.book.with(|w| w.snap.accounts[0].needs_login = false);
    r.card("move_all", json!({"to": "main"}));
}

#[test]
fn an_account_in_use_that_is_not_listed_still_has_its_owner_checked() {
    let r = rig("active-unlisted");
    r.book.with(|w| w.snap.active = Some(A8.to_string()));
    assert_eq!(
        refused(&r, "switch", json!({"account": r.acct(A2)})),
        "unknown_owner"
    );
    assert_eq!(
        refused(&r, "move_all", json!({"to": "main"})),
        "unknown_owner"
    );
    r.book
        .with(|w| w.providers[0].1.active_id = Some("c8c8-0008".to_string()));
    assert_eq!(
        refused(&r, "switch", json!({"account": r.codex(C2)})),
        "unknown_owner"
    );
    assert_eq!(r.open_count(), 0);
}

#[test]
fn another_organizations_codex_id_is_never_before_it_is_told_apart() {
    let r = rig("codex-lanes");
    r.book.with(|w| {
        w.providers[0]
            .1
            .accounts
            .push(ProviderAccount::new(C3, "fay@else.com", None));
        w.listing[1]
            .1
            .push((C3.to_string(), Some(OTHER.to_string())));
    });
    let theirs = r.codex(C3);
    let mine = r.codex(C2);
    for (action, other, own) in [
        (
            "lane_create",
            json!({"account": theirs}),
            json!({"account": mine}),
        ),
        (
            "lane_switch",
            json!({"lane": r.lane("cat"), "account": theirs}),
            json!({"lane": r.lane("cat"), "account": mine}),
        ),
        ("move_all", json!({"to": theirs}), json!({"to": mine})),
    ] {
        let err = r.ask(action, other).unwrap_err();
        assert_eq!(reason(&err), "never", "{action}");
        assert_eq!(err.message, NEVER);
        let err = r.ask(action, own).unwrap_err();
        assert_eq!(reason(&err), "bad_value", "{action}");
        assert_eq!(err.message, "Only Claude Code accounts have lanes.");
    }
    assert_eq!(r.open_count(), 0);
}

#[test]
fn an_add_with_a_closed_view_or_no_listing_places_nothing() {
    for (tag, close) in [("add-closed", true), ("add-unlisted", false)] {
        let r = rig(tag);
        let id = r.card("add", json!({"tool": "claude"}));
        r.apply(&id, json!({"login": "L1"})).unwrap();
        r.book.with(|w| {
            if close {
                w.closed = true;
            } else {
                w.listing.retain(|(k, _)| *k != Kind::Claude);
            }
        });
        r.asks.follow().saw(
            topics::ACCOUNTS,
            topics::ACCOUNTS_ADD_ENDED,
            &json!({"login": "L1", "state": "added",
                "account": {"id": "a9a9:0909", "email": "new@work.com"}}),
        );
        assert_eq!(r.state(&id), "applied", "{tag}");
        assert_eq!(r.row(&id).result.unwrap()["account"], Value::Null, "{tag}");
        assert!(r.book.with(|w| w.placed.is_empty()), "{tag}");
    }
}

fn signed_in_to(r: &Rig, raw: &str) -> Value {
    signed_in(r, raw, true)
}

fn signed_in_unlisted(r: &Rig, raw: &str) -> Value {
    signed_in(r, raw, false)
}

fn signed_in(r: &Rig, raw: &str, listed: bool) -> Value {
    let id = r.card("add", json!({"tool": "claude"}));
    r.apply(&id, json!({"login": "L1"})).unwrap();
    if listed {
        r.list_new(Kind::Claude, raw);
    }
    r.asks.follow().saw(
        topics::ACCOUNTS,
        topics::ACCOUNTS_ADD_ENDED,
        &json!({"login": "L1", "state": "added", "account": {"id": raw}}),
    );
    assert_eq!(r.state(&id), "applied");
    r.row(&id).result.unwrap()["account"].clone()
}

#[test]
fn an_add_that_ends_on_an_account_known_before_moves_nothing() {
    let r = rig("add-known");
    assert_eq!(signed_in_to(&r, A7), Value::Null, "a Personal account");
    assert_eq!(signed_in_to(&r, A4), Value::Null, "another organization's");
    assert_eq!(signed_in_unlisted(&r, A5), Value::Null, "an unlisted one");
    assert_eq!(signed_in_to(&r, A2), json!(r.acct(A2)), "already ours");
    assert!(r.book.with(|w| w.placed.is_empty()));
    assert_eq!(signed_in_to(&r, A8), json!(r.acct(A8)), "a new one");
    assert_eq!(
        r.book.with(|w| w.placed.clone()),
        vec![(Kind::Claude, A8.to_string(), ORG.to_string())]
    );

    let r = rig("add-known-codex");
    r.book.with(|w| {
        let codex = &mut w.providers[0].1;
        codex
            .accounts
            .push(ProviderAccount::new(C3, "fay@home.com", None));
        codex.active_id = Some(C3.to_string());
        w.listing[1].1.push((C3.to_string(), None));
    });
    let id = r.card("add", json!({"tool": "codex"}));
    r.apply(&id, json!({"state": "waiting", "attempt": 2}))
        .unwrap();
    r.add_ends(2, &[]);
    r.add_changed("codex", 2, "done");
    assert_eq!(r.state(&id), "applied");
    assert_eq!(r.row(&id).result.unwrap()["account"], Value::Null);
    assert!(r.book.with(|w| w.placed.is_empty()));
}

#[test]
fn an_add_that_ends_on_the_login_in_use_or_a_lanes_account_moves_nothing() {
    let r = rig("add-active");
    r.book.with(|w| w.snap.active = Some(A8.to_string()));
    assert_eq!(signed_in_to(&r, A8), Value::Null);
    assert!(r.book.with(|w| w.placed.is_empty()));

    let r = rig("add-lane");
    r.book.with(|w| w.lanes[0].account_id = A9.to_string());
    assert_eq!(signed_in_to(&r, A9), Value::Null);
    assert!(r.book.with(|w| w.placed.is_empty()));
}

#[test]
fn a_new_account_that_is_not_listed_is_never_taken_as_personal() {
    let r = rig("add-new-unlisted");
    assert_eq!(signed_in_unlisted(&r, A9), Value::Null);
    assert!(r.book.with(|w| w.placed.is_empty()));
    let r = rig("add-new-unlisted-personal");
    r.who.lock().unwrap().organization = Some(PERSONAL_ID.to_string());
    assert_eq!(signed_in_unlisted(&r, A9), Value::Null);
    assert!(r.book.with(|w| w.placed.is_empty()));
}

#[test]
fn a_session_neither_on_a_lane_nor_on_main_is_not_moved() {
    let r = rig("move-elsewhere");
    r.book.with(|w| {
        w.agents
            .push(agent("agent-5", true, "/home/u/.claude-other", HERE));
        let mut loose = agent("agent-6", true, "/home/u/.claude-other", HERE);
        loose.config_dir = None;
        w.agents.push(loose);
    });
    for session in ["agent-5", "agent-6"] {
        assert_eq!(
            refused(&r, "move", json!({"session": session, "to": r.lane("cat")})),
            "unknown_owner",
            "{session}"
        );
    }
    assert_eq!(r.open_count(), 0);
    r.card("move", json!({"session": "agent-1", "to": r.lane("cat")}));
}

#[test]
fn an_add_that_stored_nothing_names_no_account_even_the_callers_own_in_use() {
    let r = rig("add-nothing-own");
    let active = r.book.with(|w| w.providers[0].1.active_id.clone());
    assert_eq!(active.as_deref(), Some(C1));
    let own = |w: &mut World| {
        let mut codex = w.listing[1].1.iter();
        codex.any(|(raw, org)| raw == C1 && org.as_deref() == Some(ORG))
    };
    assert!(r.book.with(own), "the login in use is the caller's own");
    let id = r.card("add", json!({"tool": "codex"}));
    r.apply(&id, json!({"state": "waiting", "attempt": 3}))
        .unwrap();
    r.add_ends(3, &[]);
    r.add_changed("codex", 3, "done");
    assert_eq!(r.state(&id), "applied");
    assert_eq!(r.row(&id).result.unwrap()["account"], Value::Null);
    assert!(r.book.with(|w| w.placed.is_empty()));
}

#[test]
fn a_claude_add_whose_reply_names_no_login_is_not_followed() {
    let r = rig("claude-no-login");
    let id = r.card("add", json!({"tool": "claude"}));
    r.apply(&id, json!({"url": "https://claude.ai/oauth?code=SECRET"}))
        .unwrap();
    assert_eq!(r.state(&id), "failed");
    assert_eq!(r.asks.follow().holding(), 0);
    assert!(r.asks.follow().is_empty());
    r.list_new(Kind::Claude, A8);
    r.asks.follow().saw(
        topics::ACCOUNTS,
        topics::ACCOUNTS_ADD_ENDED,
        &json!({"login": "L0", "state": "added", "account": {"id": A8}}),
    );
    assert_eq!(r.state(&id), "failed");
    assert!(r.book.with(|w| w.placed.is_empty()));
}
