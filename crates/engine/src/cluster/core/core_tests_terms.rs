use serde_json::{json, Value};

use super::{Effect, Input, LocalCmd, NodeCore, CALL_MS, DEDUPE_MS, PING_MS};
use crate::cluster::id::Tool;
use crate::cluster::members::Term;
use crate::cluster::testkit::{id, versions};
use crate::cluster::wire::{Msg, Origin, OriginTier};

use super::core_tests::{answers, dispatch_cmd, pair, run_locals};

#[test]
fn call_reply_correlates_and_times_out() {
    let (mut hub, m5, w1) = pair();
    let origin = Origin {
        node: m5.clone(),
        tier: OriginTier::Node,
    };
    let effects = hub.local(
        &m5,
        LocalCmd::Forward {
            node: w1.clone(),
            op: "node.load".to_string(),
            args: json!({}),
            origin: origin.clone(),
        },
    );
    assert!(answers(&effects).is_empty());
    let runs = run_locals(
        &hub.seen
            .iter()
            .filter(|(n, _)| n == "w1")
            .map(|(_, e)| e.clone())
            .collect::<Vec<_>>(),
    );
    let (call, op, _) = runs.last().cloned().expect("run local");
    assert_eq!(op, "node.load");
    let effects = hub.run(
        &w1,
        Input::LocalDone {
            call,
            result: Ok(json!({"cpu": 3})),
        },
    );
    assert!(effects.iter().any(
        |e| matches!(e, Effect::Send { msg: Msg::Reply { ok: Some(v), .. }, .. } if v["cpu"] == 3)
    ));
    let answered: Vec<_> = hub
        .seen
        .iter()
        .filter(|(n, e)| {
            n == "m5" && matches!(e, Effect::Answer { result: Ok(v), .. } if v["cpu"] == 3)
        })
        .collect();
    assert_eq!(answered.len(), 1);

    let effects = hub.local(
        &m5,
        LocalCmd::Forward {
            node: w1.clone(),
            op: "node.load".to_string(),
            args: json!({}),
            origin,
        },
    );
    assert!(answers(&effects).is_empty());
    let start = hub.seen.len();
    for _ in 0..(CALL_MS / PING_MS) {
        hub.tick_all(PING_MS);
    }
    let timeouts: Vec<_> = hub.seen[start..]
        .iter()
        .filter(
            |(_, e)| matches!(e, Effect::Answer { result: Err(err), .. } if err.code == "timeout"),
        )
        .collect();
    assert_eq!(timeouts.len(), 1);
}

#[test]
fn incoming_call_from_worker_to_dispatch_is_refused() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Call {
                id: 9,
                op: "session.new".to_string(),
                args: json!({}),
                origin: Origin {
                    node: w1.clone(),
                    tier: OriginTier::Node,
                },
                term: hub.core(&m5).highest_term().cloned().expect("term"),
            },
        },
    );
    assert!(effects.iter().any(|e| matches!(e, Effect::Send { msg: Msg::Reply { err: Some(e), .. }, .. } if e.code == "forbidden")));
    assert!(effects
        .iter()
        .any(|e| matches!(e, Effect::Publish { name, .. } if name == "cluster.refused")));
    assert!(run_locals(&effects).is_empty());
}

#[test]
fn duplicate_call_runs_once() {
    let (mut hub, m5, w1) = pair();
    let term = hub.core(&w1).highest_term().cloned().expect("term");
    let call = Msg::Call {
        id: 5,
        op: "session.new".to_string(),
        args: json!({"cwd": "/x"}),
        origin: Origin {
            node: m5.clone(),
            tier: OriginTier::Node,
        },
        term,
    };
    let first = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: call.clone(),
        },
    );
    let runs = run_locals(&first);
    assert_eq!(runs.len(), 1);
    let again = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: call.clone(),
        },
    );
    assert!(run_locals(&again).is_empty());
    hub.run(
        &w1,
        Input::LocalDone {
            call: runs[0].0,
            result: Ok(json!({"session": "s"})),
        },
    );
    let third = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: call,
        },
    );
    assert!(run_locals(&third).is_empty());
    assert!(third.iter().any(|e| matches!(
        e,
        Effect::Send {
            msg: Msg::Reply {
                ok: Some(_),
                id: 5,
                ..
            },
            ..
        }
    )));
}

#[test]
fn dedupe_forgets_after_its_window() {
    let (mut hub, m5, w1) = pair();
    let term = hub.core(&w1).highest_term().cloned().expect("term");
    let call = Msg::Call {
        id: 5,
        op: "session.new".to_string(),
        args: json!({}),
        origin: Origin {
            node: m5.clone(),
            tier: OriginTier::Node,
        },
        term,
    };
    let first = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: call.clone(),
        },
    );
    let runs = run_locals(&first);
    hub.run(
        &w1,
        Input::LocalDone {
            call: runs[0].0,
            result: Ok(json!({})),
        },
    );
    for _ in 0..70 {
        hub.tick_all(5_000);
    }
    assert!(hub.now > DEDUPE_MS);
    let later = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: call,
        },
    );
    assert_eq!(run_locals(&later).len(), 1);
}

#[test]
fn older_term_call_is_refused() {
    let (mut hub, m5, w1) = pair();
    let newer = Term {
        n: 3,
        node: id("w1"),
    };
    hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::HelloOk {
                node: m5.clone(),
                cluster: "c1".to_string(),
                term: newer.clone(),
                home: id("w1"),
            },
        },
    );
    assert_eq!(hub.core(&w1).highest_term(), Some(&newer));
    let effects = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::Call {
                id: 11,
                op: "session.new".to_string(),
                args: json!({}),
                origin: Origin {
                    node: m5.clone(),
                    tier: OriginTier::Node,
                },
                term: Term {
                    n: 1,
                    node: id("m5"),
                },
            },
        },
    );
    assert!(run_locals(&effects).is_empty());
    let reply = effects.iter().find_map(|e| match e {
        Effect::Send {
            msg: Msg::Reply { err: Some(err), .. },
            ..
        } => Some(err.clone()),
        _ => None,
    });
    let err = reply.expect("a reply");
    assert_eq!(err.code, "stale_term");
    let body: Value = serde_json::from_str(&err.message).expect("json");
    assert_eq!(body["term"]["n"], 3);
    assert_eq!(body["home"], json!(id("w1")));
}

#[test]
fn newer_term_is_adopted() {
    let (mut hub, m5, w1) = pair();
    let newer = Term {
        n: 2,
        node: id("w1"),
    };
    let effects = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::Ping { boot_ms: 1 },
        },
    );
    assert!(!effects.is_empty());
    let effects = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::Call {
                id: 2,
                op: "node.load".to_string(),
                args: json!({}),
                origin: Origin {
                    node: m5.clone(),
                    tier: OriginTier::Node,
                },
                term: newer.clone(),
            },
        },
    );
    assert_eq!(hub.core(&w1).highest_term(), Some(&newer));
    assert_eq!(hub.core(&w1).state().expect("state").home, id("w1"));
    assert!(effects.iter().any(|e| matches!(e, Effect::Persist { .. })));
}

#[test]
fn home_seeing_newer_term_steps_down() {
    let (mut hub, m5, w1) = pair();
    let newer = Term {
        n: 2,
        node: id("w1"),
    };
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Refused {
                reason: "stale_term".to_string(),
                term: Some(newer.clone()),
                home: Some(id("w1")),
            },
        },
    );
    assert!(effects.iter().any(
        |e| matches!(e, Effect::SteppedDown { term, home } if *term == newer && *home == id("w1"))
    ));
    assert!(!hub.core(&m5).is_home());
    let out = hub.local(&m5, dispatch_cmd("k", "w1", Tool::Codex));
    assert!(run_locals(&out).is_empty());
    assert!(!out.iter().any(|e| matches!(
        e,
        Effect::Send {
            msg: Msg::Members { .. },
            ..
        }
    )));
}

#[test]
fn highest_term_survives_restart() {
    let (mut hub, m5, w1) = pair();
    let newer = Term {
        n: 4,
        node: id("m5"),
    };
    hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::HelloOk {
                node: m5.clone(),
                cluster: "c1".to_string(),
                term: newer.clone(),
                home: id("m5"),
            },
        },
    );
    let saved = hub.core(&w1).state().cloned().expect("state");
    let mut again = NodeCore::new(w1.clone(), versions("linux"), Some(saved), false);
    assert_eq!(again.highest_term(), Some(&newer));
    let effects = again.step(Input::Msg {
        from: m5.clone(),
        msg: Msg::Call {
            id: 1,
            op: "node.load".to_string(),
            args: json!({}),
            origin: Origin {
                node: m5.clone(),
                tier: OriginTier::Node,
            },
            term: Term {
                n: 1,
                node: id("m5"),
            },
        },
    });
    assert!(run_locals(&effects).is_empty());
}
