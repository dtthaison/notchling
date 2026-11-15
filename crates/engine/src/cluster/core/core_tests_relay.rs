use serde_json::json;

use super::core_tests::{answers, dispatch_cmd, Hub};
use super::core_tests_adopt::{follows_x_with_its_members, newer, relink, stranded};
use super::{Effect, Input, LocalCmd, PEER_STALE_TERM};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::members::Term;
use crate::cluster::wire::{Msg, Origin, OriginTier, WireError};

fn trio() -> (Hub, NodeId, NodeId, NodeId) {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.join("m5", "w1", "linux", false);
    (hub, m5, m1, w1)
}

fn worker_term(w: &NodeId) -> Term {
    Term {
        n: 2,
        node: w.clone(),
    }
}

fn naming(term: &Term) -> String {
    json!({"term": term, "home": term.node}).to_string()
}

fn node_origin(node: &NodeId) -> Origin {
    Origin {
        node: node.clone(),
        tier: OriginTier::Node,
    }
}

fn load_call(hub: &mut Hub, from: &NodeId, to: &NodeId) -> Vec<Effect> {
    hub.local(
        from,
        LocalCmd::Forward {
            node: to.clone(),
            op: "node.load".to_string(),
            args: json!({}),
            origin: node_origin(from),
        },
    )
}

fn call_to(hub: &Hub, who: &str, to: &NodeId) -> (u64, Msg) {
    hub.seen
        .iter()
        .rev()
        .find_map(|(n, e)| match e {
            Effect::Send {
                to: peer,
                msg: msg @ Msg::Call { id, .. },
            } if n == who && peer == to => Some((*id, msg.clone())),
            _ => None,
        })
        .expect("a call went out")
}

fn replies(hub: &Hub, from: &str, to: &NodeId) -> Vec<WireError> {
    hub.seen
        .iter()
        .filter_map(|(n, e)| match e {
            Effect::Send {
                to: peer,
                msg: Msg::Reply { err: Some(err), .. },
            } if n == from && peer == to => Some(err.clone()),
            _ => None,
        })
        .collect()
}

fn lie(hub: &mut Hub, to: &NodeId, from: &NodeId, id: u64, code: &str, term: &Term) {
    hub.run(
        to,
        Input::Msg {
            from: from.clone(),
            msg: Msg::Reply {
                id,
                ok: None,
                err: Some(WireError::new(code, naming(term))),
            },
        },
    );
}

fn unmoved(hub: &Hub, who: &NodeId, home: &NodeId) {
    let state = hub.core(who).state().expect("state");
    assert_eq!(state.home, *home);
    assert_eq!(state.term.n, 1);
    assert_eq!(state.term.node, *home);
}

#[test]
fn a_worker_naming_itself_in_a_stale_term_reply_to_a_dispatch_cannot_move_the_caller() {
    let (mut hub, m5, m1, w1) = trio();
    let started = hub.local(&m1, dispatch_cmd("k", "w1", Tool::Agy));
    assert!(answers(&started).is_empty());
    let (call, _) = call_to(&hub, "m5", &w1);
    lie(&mut hub, &m5, &w1, call, "stale_term", &worker_term(&w1));
    unmoved(&hub, &m5, &m5);
    unmoved(&hub, &m1, &m5);
    unmoved(&hub, &w1, &m5);
    let relayed = replies(&hub, "m5", &m1);
    assert_eq!(relayed.len(), 1, "{relayed:?}");
    assert_eq!(relayed[0].code, PEER_STALE_TERM);
    assert_ne!(relayed[0].code, "stale_term");
}

#[test]
fn the_caller_hears_the_workers_failure_under_the_relay_code() {
    let (mut hub, m5, m1, w1) = trio();
    hub.local(&m1, dispatch_cmd("k", "w1", Tool::Agy));
    let (call, _) = call_to(&hub, "m5", &w1);
    lie(&mut hub, &m5, &w1, call, "stale_term", &worker_term(&w1));
    let heard: Vec<&WireError> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "m1")
        .filter_map(|(_, e)| match e {
            Effect::Answer { result: Err(e), .. } => Some(e),
            _ => None,
        })
        .collect();
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0].code, PEER_STALE_TERM);
    let body: serde_json::Value = serde_json::from_str(&heard[0].message).expect("the text stays");
    assert_eq!(body["term"], json!(worker_term(&w1)));
}

#[test]
fn a_worker_cannot_move_the_home_by_answering_a_forwarded_call() {
    let (mut hub, m5, _m1, w1) = trio();
    load_call(&mut hub, &m5, &w1);
    let (call, _) = call_to(&hub, "m5", &w1);
    lie(&mut hub, &m5, &w1, call, "stale_term", &worker_term(&w1));
    unmoved(&hub, &m5, &m5);
    let heard = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "m5")
        .filter(|(_, e)| matches!(e, Effect::Answer { result: Err(_), .. }))
        .count();
    assert_eq!(heard, 1);
}

#[test]
fn a_local_run_that_ends_in_a_stale_term_error_is_relayed_under_the_relay_code() {
    let (mut hub, m5, m1, w1) = trio();
    load_call(&mut hub, &m1, &m5);
    let run = hub
        .seen
        .iter()
        .rev()
        .find_map(|(n, e)| match e {
            Effect::RunLocal { call, .. } if n == "m5" => Some(*call),
            _ => None,
        })
        .expect("the home ran the call");
    hub.run(
        &m5,
        Input::LocalDone {
            call: run,
            result: Err(WireError::new("stale_term", naming(&worker_term(&w1)))),
        },
    );
    unmoved(&hub, &m1, &m5);
    let relayed = replies(&hub, "m5", &m1);
    assert_eq!(relayed.len(), 1, "{relayed:?}");
    assert_eq!(relayed[0].code, PEER_STALE_TERM);
}

#[test]
fn a_repeated_call_replays_the_relay_code_from_the_cache() {
    let (mut hub, m5, m1, w1) = trio();
    hub.local(&m1, dispatch_cmd("k", "w1", Tool::Agy));
    let (on_the_wire, original) = call_to(&hub, "m1", &m5);
    let (call, _) = call_to(&hub, "m5", &w1);
    lie(&mut hub, &m5, &w1, call, "stale_term", &worker_term(&w1));
    assert_eq!(replies(&hub, "m5", &m1).len(), 1);
    let again = hub.run(
        &m5,
        Input::Msg {
            from: m1.clone(),
            msg: original,
        },
    );
    let replayed: Vec<&WireError> = again
        .iter()
        .filter_map(|e| match e {
            Effect::Send {
                msg: Msg::Reply {
                    id, err: Some(err), ..
                },
                ..
            } if *id == on_the_wire => Some(err),
            _ => None,
        })
        .collect();
    assert_eq!(replayed.len(), 1, "{again:?}");
    assert_eq!(replayed[0].code, PEER_STALE_TERM);
    unmoved(&hub, &m1, &m5);
}

#[test]
fn the_homes_own_stale_term_still_reaches_the_caller_and_moves_it() {
    let mut c = stranded();
    let (n, h) = (c.n.clone(), c.h.clone());
    load_call(&mut c.hub, &n, &h);
    let own = replies(&c.hub, "m5", &n);
    let last = own.last().expect("the old home answered");
    assert_eq!(last.code, "stale_term");
    let body: serde_json::Value = serde_json::from_str(&last.message).expect("the term and home");
    assert_eq!(body["term"], json!(newer()));
    let state = c.hub.core(&n).state().expect("state");
    assert_eq!(state.home, c.x);
    assert_eq!(state.term, newer());
    relink(&mut c.hub, &n, &c.x);
    assert!(follows_x_with_its_members(&c));
}
