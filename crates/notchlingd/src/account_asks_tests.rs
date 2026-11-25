use super::rig::*;
use super::*;
use crate::consent_testkit::{agent_caller, app_caller, shell, verified};
use notchling_engine::consent::CLUSTER;
use notchling_engine::organizations::PERSONAL_ID;
use notchling_local_api::topics;
use std::sync::atomic::Ordering;

type Open = Box<dyn Fn(&Rig) -> String>;

fn table(r: &Rig) -> Vec<(&'static str, Value, &'static str, Value)> {
    vec![
        (
            "switch",
            json!({"account": r.acct(A2)}),
            ops::ACCOUNTS_SWITCH,
            json!({"id": A2}),
        ),
        (
            "switch",
            json!({"account": r.codex(C2)}),
            ops::PROVIDERS_SWITCH,
            json!({"provider": "codex", "id": C2, "confirm": true}),
        ),
        (
            "add",
            json!({"tool": "codex"}),
            ops::PROVIDERS_ADD_BEGIN,
            json!({"provider": "codex", "confirm": true}),
        ),
        (
            "remove",
            json!({"account": r.acct(A2)}),
            ops::ACCOUNTS_REMOVE,
            json!({"id": A2}),
        ),
        (
            "remove",
            json!({"account": r.codex(C2)}),
            ops::PROVIDERS_REMOVE,
            json!({"provider": "codex", "id": C2}),
        ),
        (
            "move_all",
            json!({"to": "main"}),
            ops::ACCOUNTS_MOVE_ALL,
            json!({"target": null}),
        ),
        (
            "move",
            json!({"session": "agent-1", "to": r.lane("cat")}),
            ops::SESSION_MOVE,
            json!({"id": "agent-1", "to_config_dir": LANE_PATH}),
        ),
        (
            "lane_create",
            json!({"account": r.acct(A2)}),
            ops::LANES_CREATE,
            json!({"account": A2}),
        ),
        (
            "lane_remove",
            json!({"lane": r.lane("cat")}),
            ops::LANES_REMOVE,
            json!({"name": "cat"}),
        ),
        (
            "lane_switch",
            json!({"lane": r.lane("cat"), "account": r.acct(A2)}),
            ops::LANES_SWITCH,
            json!({"name": "cat", "account": A2}),
        ),
    ]
}

#[test]
fn every_action_asks_first_and_runs_only_when_the_app_allows() {
    let base = rig("each-base");
    for (n, (action, args, op, raw)) in table(&base).into_iter().enumerate() {
        let r = rig(&format!("each-{n}"));
        if action == "lane_remove" {
            r.book.with(|w| w.agents.retain(|a| a.id != "agent-7"));
        }
        let args = serde_json::from_str::<Value>(
            &args
                .to_string()
                .replace(&base.acct(A2), &r.acct(A2))
                .replace(&base.codex(C2), &r.codex(C2))
                .replace(&base.lane("cat"), &r.lane("cat")),
        )
        .unwrap();
        let id = r.card(action, args);
        let row = r.row(&id);
        assert_eq!(row.kind, ACCOUNT, "{action}");
        assert_eq!(row.change["action"], action);
        assert_eq!(row.change["apply"]["op"], op, "{action}");
        assert!(r.ran().is_empty(), "asking runs nothing");
        let reply = match action {
            "add" => json!({"state": "waiting", "attempt": 1}),
            _ => json!({}),
        };
        r.apply(&id, reply).unwrap();
        assert_eq!(r.ran(), vec![(op.to_string(), raw.clone())], "{action}");
        if action == "add" {
            assert_eq!(r.state(&id), "applying");
            r.add_changed("codex", 1, "done");
        }
        assert_eq!(r.state(&id), "applied", "{action} {op}");
        assert!(r.asks.follow().is_empty(), "{action}");
        assert_eq!(r.asks.follow().holding(), 0, "{action}");
    }
}

#[test]
fn another_organizations_account_is_refused_with_never_before_a_card_for_every_action() {
    let r = rig("never");
    let other = r.acct(A4);
    let asks = vec![
        ("switch", json!({"account": other})),
        ("remove", json!({"account": other})),
        ("move_all", json!({"to": other})),
        ("lane_create", json!({"account": other})),
        (
            "lane_switch",
            json!({"lane": r.lane("cat"), "account": other}),
        ),
    ];
    for (action, args) in asks {
        let refused = r.ask(action, args).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden, "{action}");
        assert_eq!(reason(&refused), "never", "{action}");
        assert_eq!(refused.data.as_ref().unwrap()["never"], true);
        assert_eq!(refused.message, NEVER);
    }
    r.book.with(|w| {
        w.agents.push(agent("agent-3", true, MAIN, THERE));
        w.agents.push(agent("agent-4", true, LANE_PATH, THERE));
        w.lanes.push(Lane {
            name: "zed".to_string(),
            path: "/home/u/.notchling/lanes/zed".to_string(),
            account_id: A4.to_string(),
            email: "zed@else.com".to_string(),
            folder: None,
        });
    });
    let u1 = vec![
        ("switch", json!({"account": r.acct(A2)})),
        ("move_all", json!({"to": "main"})),
        ("move_all", json!({"to": r.acct(A3)})),
        (
            "lane_switch",
            json!({"lane": r.lane("cat"), "account": r.acct(A2)}),
        ),
        ("move", json!({"session": "agent-3", "to": r.lane("cat")})),
        ("move", json!({"session": "agent-1", "to": r.lane("zed")})),
        ("lane_remove", json!({"lane": r.lane("zed")})),
    ];
    for (action, args) in u1 {
        let refused = r.ask(action, args.clone()).unwrap_err();
        assert_eq!(reason(&refused), "never", "{action} {args}");
    }
    assert_eq!(r.open_count(), 0);
    assert!(r.ran().is_empty());
    assert!(r.sink.events().is_empty(), "{:?}", r.sink.events());
    r.inbox.flush();
    assert!(r.inbox.items().is_empty());
}

#[test]
fn an_account_whose_owner_is_unknown_is_refused_never_taken_for_personal() {
    let r = rig("unknown");
    for raw in [A5, A6] {
        let refused = r
            .ask("switch", json!({"account": r.acct(raw)}))
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden);
        assert_eq!(reason(&refused), "unknown_owner", "{raw}");
    }
    let personal = r.ask("switch", json!({"account": r.acct(A7)})).unwrap_err();
    assert_eq!(
        reason(&personal),
        "never",
        "a listed account with no field is Personal"
    );
    r.book.with(|w| w.closed = true);
    let closed = r.ask("switch", json!({"account": r.acct(A2)})).unwrap_err();
    assert_eq!(reason(&closed), "unknown_owner");
    r.book.with(|w| {
        w.closed = false;
        w.listing.retain(|(k, _)| *k != Kind::Codex);
    });
    let none = r
        .ask("remove", json!({"account": r.codex(C2)}))
        .unwrap_err();
    assert_eq!(reason(&none), "unknown_owner", "a listing that is None");
    assert_eq!(r.open_count(), 0);

    let mut me = verified("parent");
    me.organization = Some(PERSONAL_ID.to_string());
    *r.who.lock().unwrap() = me;
    let unlisted = r
        .ask("lane_create", json!({"account": r.acct(A5)}))
        .unwrap_err();
    assert_eq!(
        reason(&unlisted),
        "unknown_owner",
        "a miss is never Personal"
    );
}

#[test]
fn a_shell_with_no_verified_session_is_refused_with_no_session_for_every_action() {
    let r = rig("shell");
    let mut idents = vec![shell("Terminal")];
    let mut no_org = verified("parent");
    no_org.organization = None;
    idents.push(no_org);
    let mut unverified = verified("parent");
    unverified.organization = Some(ORG.to_string());
    unverified.verified = false;
    idents.push(unverified);
    for who in idents {
        *r.who.lock().unwrap() = who;
        for (action, args, ..) in table(&r) {
            let refused = r.ask(action, args).unwrap_err();
            assert_eq!(refused.code, ErrorCode::Forbidden, "{action}");
            assert_eq!(reason(&refused), "no_session", "{action}");
            assert_eq!(refused.message, NO_SESSION);
        }
    }
    assert_eq!(r.open_count(), 0);
}

#[test]
fn the_accounts_switch_must_be_on_and_asks_for_it() {
    let r = rig("off");
    r.book.with(|w| w.accounts_on = false);
    for (action, args, ..) in table(&r) {
        let refused = r.ask(action, args).unwrap_err();
        assert_eq!(reason(&refused), "switch_off", "{action}");
        let data = refused.data.unwrap();
        assert_eq!(data["switch"], "accounts");
        assert_eq!(data["ask"]["command"], "notchling switch on accounts");
    }
    assert_eq!(r.open_count(), 0);
}

#[test]
fn moving_needs_acting_on() {
    let r = rig("acting");
    r.book.with(|w| w.acting_on = false);
    let refused = r
        .ask("move", json!({"session": "agent-1", "to": r.lane("cat")}))
        .unwrap_err();
    assert_eq!(reason(&refused), "switch_off");
    assert_eq!(refused.data.unwrap()["switch"], "acting");
    let all = r.ask("move_all", json!({"to": "main"})).unwrap_err();
    assert_eq!(reason(&all), "switch_off", "lane jobs would move");
    r.book.with(|w| w.agents.retain(|a| a.id != "agent-7"));
    let id = r.card("move_all", json!({"to": r.acct(A2)}));
    assert_eq!(r.row(&id).change["jobs"], json!([]));
}

#[test]
fn dry_run_opens_nothing_runs_nothing_follows_nothing() {
    let r = rig("dry");
    for (action, args, ..) in table(&r) {
        if action == "lane_remove" {
            continue;
        }
        let reply = r
            .asks
            .call(
                &agent_caller(),
                ops::ACCOUNTS_REQUEST,
                &json!({"action": action, "args": args, "dryRun": true,
                    "consent": {"key": "k1"}}),
            )
            .unwrap();
        assert_eq!(reply["dryRun"], true, "{action}");
        assert_eq!(reply["would"], "ask");
        assert_eq!(reply["change"]["action"], action);
        assert!(reply["error"].is_null());
    }
    assert_eq!(r.open_count(), 0);
    assert!(r.ran().is_empty());
    assert!(r.asks.follow().is_empty());
    assert!(r.sink.events().is_empty());
    let id = r.card("switch", json!({"account": r.acct(A2)}));
    assert!(!id.is_empty(), "the key of a dry run is not taken");
}

#[test]
fn the_key_is_looked_up_before_planning_and_replays_the_first_request() {
    let r = rig("key");
    let ask = |args: Value, why: &str| {
        r.asks.call(
            &agent_caller(),
            ops::ACCOUNTS_REQUEST,
            &json!({"action": "switch", "args": args,
                "consent": {"key": "k1", "reason": why}}),
        )
    };
    let first = ask(json!({"account": r.acct(A2)}), "first").unwrap();
    r.book.calls.store(0, Ordering::SeqCst);
    let again = ask(json!({"account": r.acct(A2)}), "a new reason").unwrap();
    assert_eq!(first["request"]["id"], again["request"]["id"]);
    assert_eq!(
        r.book.calls.load(Ordering::SeqCst),
        0,
        "no planning on a replay"
    );
    r.book.with(|w| w.closed = true);
    let replay = ask(json!({"account": r.acct(A2)}), "first").unwrap();
    assert_eq!(first["request"]["id"], replay["request"]["id"]);
    let reused = ask(json!({"account": r.acct(A3)}), "first").unwrap_err();
    assert_eq!(reason(&reused), "key_reused");
    assert_eq!(reused.message, KEY_REUSED);
    assert_eq!(r.open_count(), 1);
}

#[test]
fn a_phone_allow_is_not_taken_and_a_phone_may_deny() {
    let r = rig("phone");
    let id = r.card("switch", json!({"account": r.acct(A2)}));
    let _ = r.consent.decide(&id, true, None, "phone", None);
    assert_eq!(r.state(&id), "waiting");
    assert!(r.ran().is_empty());
    let denied = r.consent.decide(&id, false, None, "phone", None).unwrap();
    assert_eq!(denied["answered"], json!(true));
    assert_eq!(r.state(&id), "denied");
    let late = r.apply(&id, json!({})).unwrap_err();
    assert_eq!(reason(&late), "needs_consent");
    assert!(r.ran().is_empty());
}

#[test]
fn allow_rechecks_local_state_and_withdraws_a_card_whose_facts_changed() {
    type Change = Box<dyn Fn(&mut World)>;
    let cases: Vec<(&str, Open, Change)> = vec![
        (
            "active changed",
            Box::new(|r: &Rig| r.card("switch", json!({"account": r.acct(A2)}))),
            Box::new(|w: &mut World| w.snap.active = Some(A7.to_string())),
        ),
        (
            "lane account changed",
            Box::new(|r: &Rig| {
                r.card(
                    "lane_switch",
                    json!({"lane": r.lane("cat"), "account": r.acct(A2)}),
                )
            }),
            Box::new(|w: &mut World| w.lanes[0].account_id = A1.to_string()),
        ),
        (
            "account moved organization",
            Box::new(|r: &Rig| r.card("lane_create", json!({"account": r.acct(A2)}))),
            Box::new(|w: &mut World| {
                w.listing[0].1[1].1 = Some(OTHER.to_string());
            }),
        ),
        (
            "session ended",
            Box::new(|r: &Rig| r.card("move", json!({"session": "agent-1", "to": r.lane("cat")}))),
            Box::new(|w: &mut World| w.agents.retain(|a| a.id != "agent-1")),
        ),
        (
            "accounts switched off",
            Box::new(|r: &Rig| r.card("remove", json!({"account": r.acct(A2)}))),
            Box::new(|w: &mut World| w.accounts_on = false),
        ),
    ];
    for (n, (what, open, change)) in cases.into_iter().enumerate() {
        let r = rig(&format!("recheck-{n}"));
        let id = open(&r);
        r.book.with(|w| change(w));
        let refused = r.apply(&id, json!({})).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden, "{what}");
        assert_eq!(reason(&refused), "stale", "{what}");
        assert!(refused.message.starts_with("It no longer fits"), "{what}");
        assert_eq!(r.state(&id), "withdrawn", "{what}");
        assert!(r.ran().is_empty(), "{what}");
    }
}

#[test]
fn the_target_is_resolved_again_right_before_the_op_runs() {
    let r = rig("resolve");
    let id = r.card("switch", json!({"account": r.acct(A2)}));
    r.book.arm(2, |w| w.snap.accounts.retain(|a| a.id != A2));
    let refused = r.apply(&id, json!({})).unwrap_err();
    assert_eq!(reason(&refused), "stale");
    assert_eq!(r.state(&id), "withdrawn");
    assert!(r.ran().is_empty());
}

#[test]
fn an_id_works_once_only_for_its_own_op_and_only_for_the_app() {
    let r = rig("once");
    let id = r.card("switch", json!({"account": r.acct(A2)}));
    let args = json!({"id": r.acct(A2), "consentId": id});
    let run = |_: &Value| Ok(json!({}));
    let agent = r
        .asks
        .apply(&agent_caller(), ops::ACCOUNTS_SWITCH, &args, &run)
        .unwrap_err();
    assert_eq!(reason(&agent), "app_only");
    let wrong_op = r
        .asks
        .apply(&app_caller(), ops::ACCOUNTS_REMOVE, &args, &run)
        .unwrap_err();
    assert_eq!(reason(&wrong_op), "needs_consent");
    let wrong_args = json!({"id": r.acct(A3), "consentId": id});
    let other = r
        .asks
        .apply(&app_caller(), ops::ACCOUNTS_SWITCH, &wrong_args, &run)
        .unwrap_err();
    assert_eq!(reason(&other), "needs_consent");
    assert_eq!(r.state(&id), "waiting");
    r.apply(&id, json!({})).unwrap();
    assert_eq!(r.state(&id), "applied");
    let twice = r.apply(&id, json!({})).unwrap_err();
    assert_eq!(reason(&twice), "needs_consent");
    assert_eq!(r.ran().len(), 1);
    assert_eq!(
        r.row(&id).result,
        Some(json!({"action": "switch", "done": true, "account": r.acct(A2)}))
    );
}

#[test]
fn a_cited_id_that_is_not_an_account_row_is_claimed_and_refused() {
    let r = rig("cited");
    for op in APP_OPS {
        assert!(r.asks.claims(op, &json!({"consentId": "c-1"})), "{op}");
        assert!(!r.asks.claims(op, &json!({})), "{op}");
    }
    assert!(!r
        .asks
        .claims(ops::CLUSTER_CREATE, &json!({"consentId": "c-1"})));
    let run = |_: &Value| Ok(json!({}));
    let gone = r
        .asks
        .apply(
            &app_caller(),
            ops::ACCOUNTS_SWITCH,
            &json!({"id": r.acct(A2), "consentId": "nope"}),
            &run,
        )
        .unwrap_err();
    assert_eq!(reason(&gone), "needs_consent");
    let opened = r
        .consent
        .open(
            &agent_caller(),
            crate::consent_ops::Opening {
                parts: None,
                kind: CLUSTER,
                target: format!("switch:{}", r.acct(A2)),
                request: json!({"op": "cluster.admin.request"}),
                change: json!({"apply": {"op": ops::ACCOUNTS_SWITCH, "args": {"id": r.acct(A2)}}}),
                facts: json!([]),
                reason: None,
                expires_in_ms: None,
                key: None,
            },
        )
        .unwrap();
    let id = opened.row().id.clone();
    let refused = r
        .asks
        .apply(
            &app_caller(),
            ops::ACCOUNTS_SWITCH,
            &json!({"id": r.acct(A2), "consentId": id}),
            &run,
        )
        .unwrap_err();
    assert_eq!(reason(&refused), "needs_consent");
    assert_eq!(refused.message, "That request is for something else.");
    assert!(r.ran().is_empty());
}

#[test]
fn each_no_op_and_precondition_refuses_with_its_reason() {
    let r = rig("noop");
    let cases: Vec<(&str, Value, &str)> = vec![
        ("switch", json!({"account": r.acct(A1)}), "already_set"),
        ("switch", json!({"account": r.acct(A3)}), "in_lane"),
        ("switch", json!({"account": r.codex(C1)}), "already_set"),
        ("remove", json!({"account": r.acct(A1)}), "in_use"),
        ("remove", json!({"account": r.acct(A3)}), "in_lane"),
        ("remove", json!({"account": r.codex(C1)}), "in_use"),
        ("move_all", json!({"to": r.codex(C2)}), "bad_value"),
        (
            "move",
            json!({"session": "agent-1", "to": "main"}),
            "already_set",
        ),
        ("lane_create", json!({"account": r.acct(A3)}), "already_set"),
        ("lane_create", json!({"account": r.acct(A1)}), "main_login"),
        ("lane_create", json!({"account": r.codex(C2)}), "bad_value"),
        ("lane_remove", json!({"lane": r.lane("cat")}), "in_use"),
        (
            "lane_switch",
            json!({"lane": r.lane("cat"), "account": r.acct(A3)}),
            "already_set",
        ),
        (
            "lane_switch",
            json!({"lane": r.lane("cat"), "account": r.acct(A1)}),
            "in_use",
        ),
        (
            "switch",
            json!({"account": format!("acct-{}", "0".repeat(16))}),
            "no_account",
        ),
        (
            "lane_remove",
            json!({"lane": format!("lane-{}", "0".repeat(16))}),
            "no_lane",
        ),
        (
            "move",
            json!({"session": "agent-404", "to": "main"}),
            "no_agent",
        ),
        ("switch", json!({"account": "ann@work.com"}), "bad_value"),
        (
            "switch",
            json!({"account": r.acct(A2), "extra": 1}),
            "usage",
        ),
        ("add", json!({"tool": "gemini"}), "bad_value"),
        (
            "move",
            json!({"session": "node:x/agent-1", "to": "main"}),
            "bad_value",
        ),
    ];
    for (action, args, why) in cases {
        let refused = r.ask(action, args.clone()).unwrap_err();
        assert_eq!(reason(&refused), why, "{action} {args}");
    }
    let bad = r.ask("rename", json!({})).unwrap_err();
    assert_eq!(reason(&bad), "usage");
    assert_eq!(bad.message, USAGE);

    r.book.with(|w| {
        w.snap.accounts[1].needs_login = true;
        w.providers[0].1.accounts[1].switchable = false;
        w.agents.push(agent("proc-1", false, MAIN, HERE));
    });
    let later: Vec<(&str, Value, &str)> = vec![
        ("switch", json!({"account": r.acct(A2)}), "signed_out"),
        ("lane_create", json!({"account": r.acct(A2)}), "signed_out"),
        (
            "lane_switch",
            json!({"lane": r.lane("cat"), "account": r.acct(A2)}),
            "signed_out",
        ),
        ("move_all", json!({"to": r.acct(A2)}), "signed_out"),
        ("switch", json!({"account": r.codex(C2)}), "bad_value"),
        (
            "move",
            json!({"session": "proc-1", "to": r.lane("cat")}),
            "bad_value",
        ),
    ];
    for (action, args, why) in later {
        let refused = r.ask(action, args.clone()).unwrap_err();
        assert_eq!(reason(&refused), why, "{action} {args}");
    }
    r.book.with(|w| {
        w.providers[0].1.accounts[1].switchable = true;
        w.providers[0].1.busy = true;
        w.providers[1].1.adding = true;
        w.agents.retain(|a| a.id != "agent-7");
    });
    let busy = r
        .ask("switch", json!({"account": r.codex(C2)}))
        .unwrap_err();
    assert_eq!(reason(&busy), "busy");
    assert_eq!(
        reason(&r.ask("add", json!({"tool": "agy"})).unwrap_err()),
        "busy"
    );
    let nothing = r.ask("move_all", json!({"to": "main"})).unwrap_err();
    assert_eq!(reason(&nothing), "already_set");
    assert_eq!(nothing.message, "Nothing would move.");
    r.book.with(|w| w.can_add = false);
    let cant = r.ask("add", json!({"tool": "codex"})).unwrap_err();
    assert_eq!(reason(&cant), "bad_value");
    assert!(r.ask("add", json!({"tool": "claude"})).is_ok());
    assert_eq!(r.open_count(), 1);
}

#[test]
fn an_add_already_followed_is_busy() {
    let r = rig("adding");
    let id = r.card("add", json!({"tool": "claude"}));
    let again = r.ask("add", json!({"tool": "claude"})).unwrap();
    assert_eq!(again["state"], "waiting", "only Allow starts a sign-in");
    r.apply(&id, json!({"login": "L1"})).unwrap();
    let busy = r.ask("add", json!({"tool": "claude"})).unwrap_err();
    assert_eq!(reason(&busy), "busy");
    let other = again["request"]["id"].as_str().unwrap().to_string();
    let refused = r.apply(&other, json!({})).unwrap_err();
    assert_eq!(reason(&refused), "stale", "rechecked at Allow");
    assert_eq!(r.state(&other), "withdrawn");
}

#[test]
fn no_row_reply_event_or_fact_holds_an_email_a_raw_id_a_lane_name_or_a_url() {
    let r = rig("secret");
    let mut ids = Vec::new();
    let mut replies = Vec::new();
    r.book.with(|w| {
        w.agents.push(agent("proc-1", false, LANE_PATH, HERE));
    });
    let asks: Vec<(&str, Value)> = vec![
        ("switch", json!({"account": r.acct(A2)})),
        ("switch", json!({"account": r.codex(C2)})),
        ("move_all", json!({"to": r.acct(A3)})),
        ("move", json!({"session": "agent-1", "to": r.lane("cat")})),
        (
            "lane_switch",
            json!({"lane": r.lane("cat"), "account": r.acct(A2)}),
        ),
        ("lane_create", json!({"account": r.acct(A2)})),
        ("remove", json!({"account": r.acct(A2)})),
        ("add", json!({"tool": "claude"})),
    ];
    for (n, (action, args)) in asks.into_iter().enumerate() {
        let mut me = verified(&format!("s{n}"));
        me.organization = Some(ORG.to_string());
        *r.who.lock().unwrap() = me;
        let dry = r
            .asks
            .call(
                &agent_caller(),
                ops::ACCOUNTS_REQUEST,
                &json!({"action": action, "args": args, "dryRun": true}),
            )
            .unwrap();
        replies.push(dry.to_string());
        let reply = r.ask(action, args).unwrap();
        replies.push(reply.to_string());
        let id = reply["request"]["id"].as_str().unwrap().to_string();
        let got = r
            .consent
            .call(&agent_caller(), ops::CONSENT_GET, &json!({"id": id}))
            .unwrap();
        replies.push(got.to_string());
        ids.push(id);
    }
    let first = ids[0].clone();
    r.apply(&first, json!({"switched": A2, "email": "bob@work.com"}))
        .unwrap();
    let add = ids.last().unwrap().clone();
    let shown = r
        .apply(
            &add,
            json!({"login": "L1", "url": "https://claude.ai/oauth?code=SECRET"}),
        )
        .unwrap();
    assert!(shown["url"].as_str().unwrap().starts_with("https://"));
    r.list_new(Kind::Claude, "a9a9:0909");
    r.asks.follow().saw(
        topics::ACCOUNTS,
        topics::ACCOUNTS_ADD_ENDED,
        &json!({"login": "L1", "state": "added",
            "account": {"id": "a9a9:0909", "email": "new@work.com"}}),
    );
    assert_eq!(r.state(&add), "applied");
    let refused = r.ask("switch", json!({"account": r.acct(A4)})).unwrap_err();
    replies.push(format!("{refused:?}"));
    let all = format!("{}|{}", r.everywhere(&ids), replies.join("|"));
    for bad in [
        "ann@",
        "bob@",
        "cat@",
        "dan@",
        "eve@",
        "zed@",
        "new@",
        A1,
        A2,
        A3,
        A4,
        C2,
        "a9a9:0909",
        "email:",
        "\"cat\"",
        "lanes/cat",
        "https://",
        "code=",
        "SECRET",
    ] {
        assert!(!all.contains(bad), "{bad} leaked");
    }
}

#[test]
fn emails_on_cards_are_masked() {
    let r = rig("mask");
    let id = r.card("switch", json!({"account": r.acct(A2)}));
    let facts = r.row(&id).facts;
    assert_eq!(
        facts,
        json!([
            {"line": "consent-account",
             "args": {"tool": "claude", "account": "b***@w***.com", "organization": ORG}},
            {"line": "consent-account-switch",
             "args": {"tool": "claude", "account": "b***@w***.com", "before": "a***@w***.com"}},
        ])
    );
    let change = r.row(&id).change;
    assert_eq!(change["account"], r.acct(A2));
    assert_eq!(change["before"], r.acct(A1));
    r.book.with(|w| w.snap.accounts[1].email = String::new());
    let id = r.card("lane_create", json!({"account": r.acct(A2)}));
    let text = r.row(&id).facts.to_string();
    assert!(
        text.contains(&r.acct(A2)),
        "an empty mask shows the id: {text}"
    );
    let codex = r.card("switch", json!({"account": r.codex(C2)}));
    let facts = r.row(&codex).facts;
    assert_eq!(facts[1]["args"]["before"], "d***@w***.com");
    assert_eq!(facts[2]["line"], "consent-account-switch-codex");
}

#[test]
fn a_move_all_carries_every_id_it_depends_on() {
    let r = rig("move-all");
    r.book.with(|w| {
        w.agents.push(agent("agent-9", true, LANE_PATH, HERE));
        w.agents.push(agent("proc-1", false, LANE_PATH, HERE));
    });
    let id = r.card("move_all", json!({"to": r.acct(A3)}));
    let row = r.row(&id);
    assert_eq!(row.target, format!("move_all:{}", r.acct(A3)));
    assert_eq!(row.change["to"], r.acct(A3));
    assert_eq!(row.change["before"], r.acct(A1));
    assert_eq!(row.change["closeLane"], r.lane("cat"));
    assert_eq!(row.change["jobs"], json!(["agent-7", "agent-9"]));
    assert_eq!(row.change["signedOut"], 1);
    let lines: Vec<&str> = row
        .facts
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["line"].as_str().unwrap())
        .collect();
    assert_eq!(
        lines,
        vec![
            "consent-account",
            "consent-account-switch",
            "consent-account-move-all",
            "consent-lane-close",
            "consent-account-signed-out"
        ]
    );
    r.apply(&id, json!({"moved": ["agent-7", "agent-9"], "errors": {}}))
        .unwrap();
    assert_eq!(r.ran()[0].1, json!({"target": A3}));
    assert_eq!(
        r.row(&id).result,
        Some(json!({"action": "move_all", "done": true, "moved": 2, "errors": 0}))
    );
}

#[test]
fn a_new_account_joins_the_callers_organization() {
    let r = rig("join");
    let id = r.card("add", json!({"tool": "claude"}));
    r.apply(&id, json!({"login": "L1"})).unwrap();
    r.list_new(Kind::Claude, "a9a9:0909");
    r.asks.follow().saw(
        topics::ACCOUNTS,
        topics::ACCOUNTS_ADD_ENDED,
        &json!({"login": "L1", "state": "added",
            "account": {"id": "a9a9:0909", "email": "new@work.com"}}),
    );
    assert_eq!(r.state(&id), "applied");
    assert_eq!(
        r.row(&id).result,
        Some(json!({"action": "add", "done": true, "tool": "claude",
            "account": r.acct("a9a9:0909")}))
    );
    let placed = r.book.with(|w| w.placed.clone());
    assert_eq!(
        placed,
        vec![(Kind::Claude, "a9a9:0909".to_string(), ORG.to_string())]
    );
}

#[test]
fn an_account_of_another_organization_signed_in_again_is_not_moved() {
    let r = rig("rejoin");
    let id = r.card("add", json!({"tool": "claude"}));
    r.apply(&id, json!({"login": "L1"})).unwrap();
    r.asks.follow().saw(
        topics::ACCOUNTS,
        topics::ACCOUNTS_ADD_ENDED,
        &json!({"login": "L1", "state": "added", "account": {"id": A4}}),
    );
    assert_eq!(r.state(&id), "applied");
    assert_eq!(r.row(&id).result.unwrap()["account"], Value::Null);
    assert!(r.book.with(|w| w.placed.is_empty()));
    let listed = r.book.with(|w| w.listing[0].1.clone());
    assert!(listed.contains(&(A4.to_string(), Some(OTHER.to_string()))));
}

#[test]
fn the_consent_event_carries_the_opaque_account_and_lane() {
    let r = rig("event");
    r.card(
        "lane_switch",
        json!({"lane": r.lane("cat"), "account": r.acct(A2)}),
    );
    let events = format!("{:?}", r.sink.events());
    assert!(!events.is_empty());
    assert!(!events.contains(A2) && !events.contains("bob@"), "{events}");
    assert!(
        events.contains("LaneSwitch") || events.contains("lane_switch"),
        "{events}"
    );
}

#[test]
fn lanes_summary_lists_only_the_callers_lanes_without_names() {
    let r = rig("lanes");
    r.book.with(|w| {
        w.lanes.push(Lane {
            name: "zed".to_string(),
            path: "/home/u/.notchling/lanes/zed".to_string(),
            account_id: A4.to_string(),
            email: "zed@else.com".to_string(),
            folder: None,
        })
    });
    let reply = r
        .asks
        .call(&agent_caller(), ops::LANES_SUMMARY, &json!({}))
        .unwrap();
    assert_eq!(
        reply,
        json!({"organization": ORG, "lanes": [{"id": r.lane("cat"), "account": r.acct(A3),
            "name": "c***@w***.com", "sessions": 1, "needsLogin": false}]})
    );
    *r.who.lock().unwrap() = shell("Terminal");
    let none = r
        .asks
        .call(&agent_caller(), ops::LANES_SUMMARY, &json!({}))
        .unwrap();
    assert_eq!(none["lanes"], json!([]));
}
