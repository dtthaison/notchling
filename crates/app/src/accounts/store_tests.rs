//! The accounts store over a fake transport that plays the daemon's replies. No socket, no
//! daemon, no login anywhere.

use super::*;
use crate::switches::ACCOUNTS_OFF;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::json;
use std::sync::Arc;

type Calls = Arc<Mutex<Vec<(String, Value)>>>;
type Script = Box<dyn Fn(&str, &Value) -> Result<Value, ClientError> + Send + Sync>;

struct Fake {
    calls: Calls,
    answer: Script,
}

impl Transport for Fake {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        self.calls
            .lock()
            .unwrap()
            .push((op.to_owned(), args.clone()));
        (self.answer)(op, &args)
    }
}

fn fake(
    answer: impl Fn(&str, &Value) -> Result<Value, ClientError> + Send + Sync + 'static,
) -> (Accounts, Calls) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let store = Accounts::over(Box::new(Fake {
        calls: calls.clone(),
        answer: Box::new(answer),
    }));
    (store, calls)
}

fn sent(calls: &Calls) -> Vec<String> {
    calls.lock().unwrap().iter().map(|c| c.0.clone()).collect()
}

/// A daemon with the switch `on`: the lists, one Codex account with a report, the watcher's
/// windows.
fn daemon(on: bool) -> impl Fn(&str, &Value) -> Result<Value, ClientError> + Send + Sync {
    move |op, args| match op {
        "accounts.list" => Ok(
            json!({"on": on, "active": if on { Some("a1") } else { None },
            "accounts": [{"id": "a1", "email": "me@example.com", "plan": "Max 20x", "needsLogin": false},
                         {"id": "a2", "email": "ops@example.com", "lane": "night"}]}),
        ),
        "lanes.list" => Ok(
            json!({"lanes": [{"name": "night", "path": "/h/.claude-lanes/night",
            "accountID": "a2", "email": "ops@example.com"}]}),
        ),
        "providers.list" => Ok(json!({"accounts_on": on, "providers": [
            {"id": "codex", "title": "Codex", "capabilities": ["addAccount", "usage"], "available": true,
             "state": {"accounts": [{"id": "u:a", "email": "me@example.com", "plan": "Plus"}],
                       "active_id": if on { Some("u:a") } else { None }}}]})),
        "providers.usage" => Ok(
            json!({"provider": "codex", "reports": {"u:a": {"fetched": 1.0,
            "groups": [{"name": "Codex", "bars": [{"label": "5h", "kind": "session", "percent": 41.0,
            "resets_at": null}]}]}}}),
        ),
        "limits.status" => Ok(json!({"accounts_on": on, "watching": on, "switch_at": 95,
            "providers": [{"id": "claude", "label": "Claude Code", "mode": "notify", "modes": [],
            "active": "me@example.com", "windows": [{"name": "5-hour", "percent": 41.0}]}]})),
        "cost.summary" => Ok(
            json!({"range": args["range"], "scanned": 1.0, "scanning": false,
            "summary": {"days": [], "today": 1.5, "week": 9.0, "month": 31.0, "models": [],
                        "model_detail": [], "projects": [], "accounts": [], "channels": [],
                        "channel_tokens": {}}}),
        ),
        _ if !on => Err(ClientError::Api(ApiError::new(
            ErrorCode::Forbidden,
            ACCOUNTS_OFF,
        ))),
        "accounts.switch" | "accounts.remove" => Ok(json!({"on": true, "active": args["id"],
            "accounts": [{"id": args["id"], "email": "x@example.com"}]})),
        "accounts.add.begin" => Ok(json!({"login": "l1", "url": "https://claude.ai/login"})),
        "accounts.add.finish" => Ok(json!({"login": "l1",
            "state": if args["cancel"] == true { "cancelled" } else { "added" },
            "account": {"id": "a3", "email": "new@example.com"}})),
        "accounts.move_all" => Ok(json!({"plan": {"jobs": ["j1"], "closeLane": "night",
            "switchMain": true, "signedOut": []}})),
        "providers.switch" => Ok(if args["confirm"] == true {
            json!({"state": "switched"})
        } else {
            json!({"state": "needs_confirmation", "confirmation": {"title": "Switch Codex to x?",
                   "message": "The ChatGPT app quits.", "button": "Switch"}})
        }),
        "providers.add.begin" => Ok(json!({"state": "waiting", "sign_in_url": "https://x"})),
        "providers.add.cancel" => Ok(json!({"cancelled": true})),
        "providers.remove" => Ok(json!({"removed": true})),
        other => Err(ClientError::Api(ApiError::bad_request(format!(
            "unknown {other}"
        )))),
    }
}

#[test]
fn a_refresh_reads_every_list_and_the_limits_only_with_the_switch_on() {
    let (store, calls) = fake(daemon(true));
    assert!(store.is_live());
    store.refresh(&Switches::all_on());
    assert_eq!(
        sent(&calls),
        [
            "accounts.list",
            "lanes.list",
            "providers.list",
            "limits.status",
            "providers.usage"
        ]
    );
    assert_eq!(calls.lock().unwrap()[0].1, json!({"sync": true}));
    let s = store.snapshot();
    assert!(s.attached);
    assert_eq!(s.claude.active.as_deref(), Some("a1"));
    assert_eq!(s.claude.accounts.len(), 2);
    assert_eq!(s.lanes[0].name, "night");
    assert_eq!(s.providers.providers[0].id, "codex");
    assert_eq!(s.usage["codex"]["u:a"].groups[0].bars[0].percent, 41.0);
    assert_eq!(s.limits.providers[0].windows[0].percent, 41.0);
    assert_eq!(s.error, None);

    // Off: the lists still show, nothing syncs and no usage is asked for.
    let (store, calls) = fake(daemon(false));
    store.refresh(&Switches::all_off());
    assert_eq!(
        sent(&calls),
        [
            "accounts.list",
            "lanes.list",
            "providers.list",
            "limits.status"
        ]
    );
    assert_eq!(calls.lock().unwrap()[0].1, json!({"sync": false}));
    let s = store.snapshot();
    assert!(s.attached && !s.claude.on && s.claude.active.is_none());
    assert_eq!(s.claude.accounts.len(), 2, "the saved names still list");
    assert!(s.usage.is_empty());
}

#[test]
fn nothing_but_the_lists_is_sent_while_the_switch_is_off() {
    let (store, calls) = fake(daemon(false));
    let off = Switches::all_off();
    assert_eq!(store.switch_claude(&off, "a2").unwrap_err(), ACCOUNTS_OFF);
    assert_eq!(store.begin_add_claude(&off).unwrap_err(), ACCOUNTS_OFF);
    assert_eq!(
        store
            .switch_provider(&off, "codex", "u:a", false)
            .unwrap_err(),
        ACCOUNTS_OFF
    );
    assert_eq!(store.move_all(&off, None, true).unwrap_err(), ACCOUNTS_OFF);
    assert!(calls.lock().unwrap().is_empty(), "refused here, unsent");
    // Detached: the daemon's absence is the reason.
    let detached = Switches::detached();
    assert_eq!(
        store.remove_claude(&detached, "a1").unwrap_err(),
        crate::acting::DETACHED_MESSAGE
    );
    assert!(store.snapshot().error.is_none());
}

#[test]
fn with_the_switch_on_the_actions_go_out_and_their_replies_land_in_the_snapshot() {
    let (store, calls) = fake(daemon(true));
    let on = Switches::all_on();
    store.switch_claude(&on, "a2").unwrap();
    assert_eq!(store.snapshot().claude.active.as_deref(), Some("a2"));
    let url = store.begin_add_claude(&on).unwrap();
    assert_eq!(url, "https://claude.ai/login");
    assert_eq!(store.snapshot().add.unwrap().state, "waiting");
    store.poll_add_claude(&on, false).unwrap();
    let add = store.snapshot().add.unwrap();
    assert_eq!(
        (add.state.as_str(), add.email.as_deref()),
        ("added", Some("new@example.com"))
    );
    store.clear_add();
    assert!(store.snapshot().add.is_none());
    store.poll_add_claude(&on, false).unwrap();
    let plan = store.move_all(&on, Some("a2"), true).unwrap();
    assert_eq!((plan.jobs, plan.close_lane.as_deref()), (1, Some("night")));
    assert_eq!(
        calls.lock().unwrap().last().unwrap().1,
        json!({"target": "a2", "dryRun": true})
    );
    let asked = store.switch_provider(&on, "codex", "u:a", false).unwrap();
    assert!(matches!(asked, Outcome::NeedsConfirmation { ref button, .. } if button == "Switch"));
    let done = store.switch_provider(&on, "codex", "u:a", true).unwrap();
    assert_eq!(
        done,
        Outcome::Done {
            state: "switched".into(),
            sign_in_url: None
        }
    );
    let adding = store.add_provider(&on, "codex", false).unwrap();
    assert!(matches!(adding, Outcome::Done { ref state, .. } if state == "waiting"));
    store.cancel_add_provider(&on, "codex").unwrap();
    store.remove_provider(&on, "codex", "u:a").unwrap();
    store.refresh_usage(&on, "codex").unwrap();
    assert!(store.snapshot().usage.contains_key("codex"));
    store.refresh_cost("7");
    let cost = store.snapshot().cost.unwrap();
    assert_eq!((cost.range.as_str(), cost.summary.week), ("7", 9.0));
}

#[test]
fn a_refusal_is_kept_as_the_error_and_a_lost_daemon_detaches() {
    let (store, _) = fake(|op, _| match op {
        "accounts.list" => Ok(json!({"on": true, "accounts": []})),
        "lanes.list" | "providers.list" | "limits.status" => {
            Err(ClientError::Api(ApiError::bad_request("no")))
        }
        _ => Err(ClientError::Api(ApiError::new(
            ErrorCode::Failed,
            "that account is in a lane",
        ))),
    });
    store.refresh(&Switches::all_on());
    assert!(store.snapshot().attached, "a refusal is still an answer");
    let err = store.switch_claude(&Switches::all_on(), "a1").unwrap_err();
    assert_eq!(err, "that account is in a lane");
    assert_eq!(
        store.snapshot().error.as_deref(),
        Some("that account is in a lane")
    );

    let (gone, _) = fake(|_, _| Err(ClientError::Closed));
    gone.refresh(&Switches::all_on());
    assert!(!gone.snapshot().attached);
    assert!(gone.switch_claude(&Switches::all_on(), "a1").is_err());
    let fixed = Accounts::fixed(Snapshot::default());
    assert!(!fixed.is_live());
    fixed.refresh(&Switches::all_on());
    assert!(fixed.begin_add_claude(&Switches::all_on()).is_err());
}

#[test]
fn a_switch_the_daemon_is_still_running_is_followed_to_its_answer() {
    let asked = Arc::new(Mutex::new(0u32));
    let polls = asked.clone();
    let (store, calls) = fake(move |op, args| match op {
        "accounts.switch" => Ok(json!({"state": "working", "ticket": 7, "op": op})),
        "accounts.result" => {
            assert_eq!(args["ticket"], 7);
            let mut n = polls.lock().unwrap();
            *n += 1;
            if *n < 2 {
                Ok(json!({"state": "working", "ticket": 7}))
            } else {
                Ok(json!({"on": true, "active": "a2",
                    "accounts": [{"id": "a2", "email": "ops@example.com"}]}))
            }
        }
        _ => Ok(json!({})),
    });
    let on = Switches::all_on();
    store.switch_claude(&on, "a2").unwrap();
    assert_eq!(store.snapshot().claude.active.as_deref(), Some("a2"));
    assert_eq!(
        sent(&calls),
        vec!["accounts.switch", "accounts.result", "accounts.result"]
    );
}

#[test]
fn a_late_refusal_is_the_error_shown() {
    let (store, _) = fake(|op, _| match op {
        "accounts.switch" => Ok(json!({"state": "working", "ticket": 3})),
        _ => Err(ClientError::Api(ApiError::new(
            ErrorCode::Failed,
            "No saved login for this account.",
        ))),
    });
    let e = store.switch_claude(&Switches::all_on(), "a9").unwrap_err();
    assert_eq!(e, "No saved login for this account.");
    assert_eq!(store.snapshot().error.as_deref(), Some(e.as_str()));
}
