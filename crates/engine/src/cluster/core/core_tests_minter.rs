use serde_json::json;

use super::core_tests::{pair, Hub};
use super::core_tests_adopt::pair_of_promotables;
use super::{Effect, Input, LocalCmd};
use crate::cluster::id::NodeId;
use crate::cluster::join::JoinToken;
use crate::cluster::testkit::{fp, id, member};
use crate::cluster::wire::{Msg, Origin, OriginTier};

struct Asked {
    can_be_master: bool,
    minter: NodeId,
    minter_name: String,
}

fn joined_with_a_token_from(hub: &mut Hub, minter: &NodeId, name: &str, os: &str) -> Asked {
    let home = id("m5");
    let secret = format!("secret-{name}");
    hub.local(
        &home,
        LocalCmd::IssueToken {
            secret: secret.clone().into(),
            minter: minter.clone(),
        },
    );
    let who = hub.add(name, os, false);
    let token = JoinToken {
        cluster: "c1".to_string(),
        home: home.clone(),
        fingerprint: fp("m5"),
        addrs: vec![],
        port: 1,
        secret,
    };
    hub.local(
        &who,
        LocalCmd::JoinWith {
            token,
            me: member(name, os, false),
        },
    );
    hub.linked.push((home.clone(), who.clone()));
    hub.run(&home, Input::Linked { peer: who.clone() });
    hub.run(&who, Input::Linked { peer: home });
    hub.seen
        .iter()
        .rev()
        .find_map(|(_, e)| match e {
            Effect::JoinRequest {
                node,
                can_be_master,
                minter,
                minter_name,
                ..
            } if *node == who => Some(Asked {
                can_be_master: *can_be_master,
                minter: minter.clone(),
                minter_name: minter_name.clone(),
            }),
            _ => None,
        })
        .expect("a join request")
}

#[test]
fn the_homes_own_token_keeps_the_default_for_a_mac() {
    let (mut hub, m5, _) = pair();
    let asked = joined_with_a_token_from(&mut hub, &m5, "mac2", "macos");
    assert!(asked.can_be_master);
    assert_eq!(asked.minter, m5);
    assert_eq!(asked.minter_name, "m5");
    let linux = joined_with_a_token_from(&mut hub, &m5, "box2", "linux");
    assert!(!linux.can_be_master);
}

#[test]
fn a_token_made_by_a_worker_never_defaults_to_can_be_master() {
    let (mut hub, m5, w1) = pair();
    let asked = joined_with_a_token_from(&mut hub, &w1, "mac2", "macos");
    assert!(!asked.can_be_master);
    assert_eq!(asked.minter, w1);
    assert_eq!(asked.minter_name, "w1");
    assert_ne!(asked.minter, m5);
}

#[test]
fn a_token_made_by_a_member_that_can_be_master_keeps_the_default_for_a_mac() {
    let (mut hub, _, w1) = pair_of_promotables();
    let asked = joined_with_a_token_from(&mut hub, &w1, "mac2", "macos");
    assert!(asked.can_be_master);
    assert_eq!(asked.minter, w1);
}

#[test]
fn a_token_whose_maker_was_removed_since_never_defaults_to_can_be_master() {
    let (mut hub, m5, w1) = pair_of_promotables();
    let secret = "late".to_string();
    hub.local(
        &m5,
        LocalCmd::IssueToken {
            secret: secret.clone().into(),
            minter: w1.clone(),
        },
    );
    hub.local(&m5, LocalCmd::Remove { node: w1.clone() });
    let who = hub.add("mac2", "macos", false);
    let token = JoinToken {
        cluster: "c1".to_string(),
        home: m5.clone(),
        fingerprint: fp("m5"),
        addrs: vec![],
        port: 1,
        secret,
    };
    hub.local(
        &who,
        LocalCmd::JoinWith {
            token,
            me: member("mac2", "macos", false),
        },
    );
    hub.linked.push((m5.clone(), who.clone()));
    hub.run(&m5, Input::Linked { peer: who.clone() });
    hub.run(&who, Input::Linked { peer: m5.clone() });
    let asked = hub
        .seen
        .iter()
        .find_map(|(_, e)| match e {
            Effect::JoinRequest {
                node,
                can_be_master,
                ..
            } if *node == who => Some(*can_be_master),
            _ => None,
        })
        .expect("a join request");
    assert!(!asked);
}

#[test]
fn only_a_member_can_be_named_as_the_maker_of_a_token() {
    let (mut hub, m5, _) = pair();
    let effects = hub.local(
        &m5,
        LocalCmd::IssueToken {
            secret: "x".to_string().into(),
            minter: id("nobody"),
        },
    );
    assert!(effects
        .iter()
        .any(|e| matches!(e, Effect::Answer { result: Err(err), .. } if err.code == "forbidden")));
    assert!(!hub.core(&m5).pin_set().1);
}

fn token_call(hub: &mut Hub, home: &NodeId, from: &NodeId, tier: OriginTier) -> Vec<Effect> {
    let term = hub.core(home).highest_term().cloned().expect("term");
    hub.nodes.get_mut(home).expect("node").step(Input::Msg {
        from: from.clone(),
        msg: Msg::Call {
            id: 3,
            op: "cluster.join.token".to_string(),
            args: json!({"minter": home}).into(),
            origin: Origin {
                node: from.clone(),
                tier,
            },
            term,
        },
    })
}

fn token_runs(effects: &[Effect]) -> Vec<(String, crate::cluster::wire::Args)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::RunLocal { op, args, .. } => Some((op.clone(), args.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_forwarded_token_call_names_the_calling_member_whatever_its_args_say() {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, false);
    let effects = token_call(&mut hub, &m5, &m1, OriginTier::App);
    let runs = token_runs(&effects);
    assert_eq!(runs.len(), 1, "{effects:?}");
    assert_eq!(runs[0].0, "cluster.join.token");
    assert_eq!(runs[0].1, json!({"minter": m1}).into());
}

#[test]
fn a_member_without_an_app_or_an_agent_gets_no_token() {
    let (mut hub, m5, w1) = pair();
    for tier in [OriginTier::App, OriginTier::Agent] {
        let effects = token_call(&mut hub, &m5, &w1, tier);
        assert!(token_runs(&effects).is_empty(), "{tier:?}: {effects:?}");
    }
}
