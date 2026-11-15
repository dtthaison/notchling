use serde_json::{json, Value};

use super::{Effect, Input, LocalCmd, NodeCore, CALL_MS, DEDUPE_MS, PING_MS, WORKSPACE_CALL_MS};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::members::{MemberState, Term};
use crate::cluster::testkit::{id, versions};
use crate::cluster::wire::{Msg, Origin, OriginTier};

use super::core_tests::{answers, dispatch_cmd, pair, run_locals, Hub};

fn promotable_pair() -> (Hub, NodeId, NodeId) {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let w1 = hub.add("w1", "macos", true);
    hub.create("m5");
    hub.join("m5", "w1", "macos", true);
    (hub, m5, w1)
}

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
    let (mut hub, m5, w1) = promotable_pair();
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
fn install_forward_from_a_primary_that_lost_its_term_is_refused() {
    let (mut hub, m5, w1) = promotable_pair();
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
    for (call_id, (op, term)) in [
        (
            "install.apply",
            Term {
                n: 1,
                node: id("m5"),
            },
        ),
        (
            "install.plan",
            Term {
                n: 1,
                node: id("m5"),
            },
        ),
        ("install.apply", newer.clone()),
        ("install.plan", newer.clone()),
    ]
    .into_iter()
    .enumerate()
    {
        let effects = hub.run(
            &w1,
            Input::Msg {
                from: m5.clone(),
                msg: Msg::Call {
                    id: 20 + call_id as u64,
                    op: op.to_string(),
                    args: json!({}),
                    origin: Origin {
                        node: m5.clone(),
                        tier: OriginTier::App,
                    },
                    term,
                },
            },
        );
        assert!(run_locals(&effects).is_empty(), "{op}");
        let refused = effects.iter().any(|e| {
            matches!(
                e,
                Effect::Send {
                    msg: Msg::Reply { err: Some(_), .. },
                    ..
                }
            )
        });
        assert!(refused, "{op}");
    }
}

#[test]
fn a_call_stamped_with_a_newer_term_is_judged_after_adopting_it() {
    for (op, tier) in [
        ("install.apply", OriginTier::App),
        ("install.plan", OriginTier::App),
        ("session.new", OriginTier::Node),
        ("shell.run", OriginTier::App),
    ] {
        let (mut hub, m5, w1) = promotable_pair();
        let newer = Term {
            n: 2,
            node: id("w1"),
        };
        let effects = hub.run(
            &w1,
            Input::Msg {
                from: m5.clone(),
                msg: Msg::Call {
                    id: 31,
                    op: op.to_string(),
                    args: json!({}),
                    origin: Origin {
                        node: m5.clone(),
                        tier,
                    },
                    term: newer.clone(),
                },
            },
        );
        assert_eq!(hub.core(&w1).highest_term(), Some(&newer), "{op}");
        assert_eq!(hub.core(&w1).state().expect("state").home, id("w1"), "{op}");
        assert!(run_locals(&effects).is_empty(), "{op} ran");
        let refused = effects.iter().any(|e| {
            matches!(
                e,
                Effect::Send {
                    msg: Msg::Reply { err: Some(_), .. },
                    ..
                }
            )
        });
        assert!(refused, "{op}");
    }
}

#[test]
fn newer_term_is_adopted() {
    let (mut hub, m5, w1) = promotable_pair();
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
    let (mut hub, m5, w1) = promotable_pair();
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

#[test]
fn workspace_calls_wait_longer_than_plain_calls() {
    let (mut hub, m5, w1) = pair();
    let origin = Origin {
        node: m5.clone(),
        tier: OriginTier::App,
    };
    let forward = |op: &str| LocalCmd::Forward {
        node: w1.clone(),
        op: op.to_string(),
        args: json!({}),
        origin: origin.clone(),
    };
    let timeouts = |hub: &super::core_tests::Hub, from: usize| {
        hub.seen[from..]
            .iter()
            .filter(
                |(_, e)| matches!(e, Effect::Answer { result: Err(err), .. } if err.code == "timeout"),
            )
            .count()
    };
    assert!(answers(&hub.local(&m5, forward("workspace.prepare"))).is_empty());
    let start = hub.seen.len();
    for _ in 0..=(CALL_MS / PING_MS) {
        hub.tick_all(PING_MS);
    }
    assert_eq!(
        timeouts(&hub, start),
        0,
        "a clone may take longer than a plain call"
    );
    for _ in 0..(WORKSPACE_CALL_MS / PING_MS) {
        hub.tick_all(PING_MS);
    }
    assert_eq!(timeouts(&hub, start), 1);
}

#[test]
fn a_workers_node_sessions_call_is_refused() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Call {
                id: 11,
                op: "node.sessions".to_string(),
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
    assert!(run_locals(&effects).is_empty());
}

fn refused_reply(effects: &[Effect]) -> bool {
    effects.iter().any(|e| {
        matches!(
            e,
            Effect::Send {
                msg: Msg::Reply { err: Some(_), .. },
                ..
            } | Effect::Send {
                msg: Msg::Refused { .. },
                ..
            }
        )
    })
}

#[test]
fn a_worker_cannot_forge_a_term_on_a_call() {
    for (op, tier) in [
        ("session.new", OriginTier::App),
        ("install.apply", OriginTier::App),
        ("node.load", OriginTier::Node),
    ] {
        let (mut hub, m5, w1) = pair();
        let forged = Term {
            n: 2,
            node: id("w1"),
        };
        let effects = hub.run(
            &m5,
            Input::Msg {
                from: w1.clone(),
                msg: Msg::Call {
                    id: 41,
                    op: op.to_string(),
                    args: json!({}),
                    origin: Origin {
                        node: w1.clone(),
                        tier,
                    },
                    term: forged,
                },
            },
        );
        assert!(run_locals(&effects).is_empty(), "{op} ran");
        assert!(refused_reply(&effects), "{op}");
        let state = hub.core(&m5).state().expect("state");
        assert_eq!(state.home, m5, "{op}");
        assert_eq!(state.term.n, 1, "{op}");
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::SteppedDown { .. })),
            "{op}"
        );
    }
}

#[test]
fn a_forged_term_on_hello_and_refused_is_not_adopted() {
    let (mut hub, m5, w1) = pair();
    let forged = Term {
        n: 5,
        node: id("w1"),
    };
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Hello {
                node: w1.clone(),
                cluster: Some("c1".to_string()),
                versions: versions("linux"),
                boot_id: "boot".to_string(),
                term: Some(forged.clone()),
            },
        },
    );
    assert!(refused_reply(&effects));
    assert_eq!(hub.core(&m5).state().expect("state").home, m5);
    hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Refused {
                reason: "stale_term".to_string(),
                term: Some(forged.clone()),
                home: Some(w1.clone()),
            },
        },
    );
    let state = hub.core(&m5).state().expect("state");
    assert_eq!(state.home, m5);
    assert_eq!(state.term.n, 1);
    let unknown = Term {
        n: 6,
        node: id("nobody"),
    };
    hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::HelloOk {
                node: w1.clone(),
                cluster: "c1".to_string(),
                term: unknown,
                home: id("nobody"),
            },
        },
    );
    assert_eq!(hub.core(&m5).state().expect("state").home, m5);
}

#[test]
fn a_forged_members_push_from_a_worker_is_not_followed() {
    let (mut hub, m5, w1) = pair();
    let state = hub.core(&m5).state().expect("state").clone();
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Members {
                term: Term {
                    n: 2,
                    node: id("w1"),
                },
                home: w1.clone(),
                vault: None,
                members: state.members.values().cloned().collect(),
                failover: None,
                armed: true,
            },
        },
    );
    assert!(refused_reply(&effects));
    let after = hub.core(&m5).state().expect("state");
    assert_eq!(after.home, m5);
    assert_eq!(after.term.n, 1);
    assert!(!effects
        .iter()
        .any(|e| matches!(e, Effect::SteppedDown { .. })));
}

#[test]
fn a_real_promotes_term_is_still_adopted_everywhere() {
    let (mut hub, m5, w1) = promotable_pair();
    let newer = Term {
        n: 2,
        node: id("w1"),
    };
    let hello_ok = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::HelloOk {
                node: w1.clone(),
                cluster: "c1".to_string(),
                term: newer.clone(),
                home: id("w1"),
            },
        },
    );
    assert!(hello_ok
        .iter()
        .any(|e| matches!(e, Effect::SteppedDown { .. })));
    assert_eq!(hub.core(&m5).state().expect("state").home, id("w1"));
}

#[test]
fn the_primarys_armed_flag_is_forgotten_with_its_term() {
    let (mut hub, m5, w1) = promotable_pair();
    let state = hub.core(&w1).state().expect("state").clone();
    let members: Vec<_> = state.members.values().cloned().collect();
    hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::Members {
                term: state.term.clone(),
                home: m5.clone(),
                vault: None,
                members,
                failover: Some(crate::cluster::failover::Failover::After30m),
                armed: true,
            },
        },
    );
    assert_eq!(hub.core(&w1).status()["failover"]["armed"], true);
    hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::HelloOk {
                node: m5.clone(),
                cluster: "c1".to_string(),
                term: Term {
                    n: 2,
                    node: id("m5"),
                },
                home: m5.clone(),
            },
        },
    );
    assert_eq!(hub.core(&w1).status()["failover"]["armed"], false);
}

fn removed_list(hub: &Hub, who: &NodeId, source: &NodeId) -> Vec<crate::cluster::members::Member> {
    let mut members: Vec<_> = hub
        .core(source)
        .state()
        .expect("state")
        .members
        .values()
        .cloned()
        .collect();
    for member in members.iter_mut().filter(|m| m.id == *who) {
        member.state = MemberState::Removed;
    }
    members
}

#[test]
fn a_forged_members_push_cannot_wipe_a_node() {
    let (mut hub, m5, w1) = pair();
    let on_primary = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Members {
                term: Term {
                    n: 2,
                    node: id("w1"),
                },
                home: w1.clone(),
                vault: None,
                members: removed_list(&hub, &m5, &m5),
                failover: None,
                armed: false,
            },
        },
    );
    assert!(refused_reply(&on_primary));
    assert!(!on_primary.iter().any(|e| matches!(e, Effect::Wipe)));
    assert_eq!(hub.core(&m5).state().expect("state").home, m5);

    let (mut hub, m5, w1) = pair();
    let w2 = hub.add("w2", "linux", false);
    hub.join("m5", "w2", "linux", false);
    hub.linked.push((w1.clone(), w2.clone()));
    hub.run(&w1, Input::Linked { peer: w2.clone() });
    hub.run(&w2, Input::Linked { peer: w1.clone() });
    let state = hub.core(&w1).state().expect("state").clone();
    assert_eq!(state.home, m5);
    let forged = hub.run(
        &w1,
        Input::Msg {
            from: w2.clone(),
            msg: Msg::Members {
                term: state.term.clone(),
                home: w2.clone(),
                vault: None,
                members: removed_list(&hub, &w1, &m5),
                failover: None,
                armed: false,
            },
        },
    );
    assert!(refused_reply(&forged), "{forged:?}");
    assert!(!forged.iter().any(|e| matches!(e, Effect::Wipe)));
    assert_eq!(hub.core(&w1).state().expect("state").home, m5);
}
