use serde_json::{json, Value};

use super::core_tests::{pair, Hub};
use super::{Effect, Input, LocalCmd};
use crate::cluster::id::NodeId;
use crate::cluster::join::{JoinToken, MAX_OPEN_TOKENS};
use crate::cluster::testkit::{fp, member, versions, versions_updated};
use crate::cluster::wire::{Msg, Origin, OriginTier};

fn report(hub: &mut Hub, home: &NodeId, from: &NodeId, id: u64, args: Value) -> Vec<Effect> {
    let term = hub.core(home).highest_term().cloned().expect("term");
    hub.nodes.get_mut(home).expect("node").step(Input::Msg {
        from: from.clone(),
        msg: Msg::Call {
            id,
            op: "node.versions".to_string(),
            args,
            origin: Origin {
                node: from.clone(),
                tier: OriginTier::Node,
            },
            term,
        },
    })
}

fn persisted(effects: &[Effect]) -> bool {
    effects.iter().any(|e| matches!(e, Effect::Persist { .. }))
}

fn pushed_members(effects: &[Effect]) -> bool {
    effects.iter().any(|e| {
        matches!(
            e,
            Effect::Send {
                msg: Msg::Members { .. },
                ..
            }
        )
    })
}

fn announced_change(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::Publish { name, .. } if name == "cluster.changed"))
}

fn replied_ok(effects: &[Effect], id: u64) -> bool {
    effects.iter().any(|e| {
        matches!(
            e,
            Effect::Send {
                msg: Msg::Reply { id: got, ok: Some(_), .. },
                ..
            } if *got == id
        )
    })
}

#[test]
fn a_member_repeating_its_versions_costs_no_journal_entry_and_no_push() {
    let (mut hub, m5, w1) = pair();
    let same = serde_json::to_value(versions("linux")).expect("versions");
    let seq = hub.core(&m5).state().expect("state").seq;
    for id in 1..=5 {
        let effects = report(&mut hub, &m5, &w1, id, same.clone());
        assert!(replied_ok(&effects, id), "{effects:?}");
        assert!(!persisted(&effects));
        assert!(!pushed_members(&effects));
        assert!(!announced_change(&effects));
    }
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq);
}

#[test]
fn a_member_reporting_new_versions_is_journaled_and_pushed_once() {
    let (mut hub, m5, w1) = pair();
    let newer = serde_json::to_value(versions_updated("linux")).expect("versions");
    let seq = hub.core(&m5).state().expect("state").seq;
    let effects = report(&mut hub, &m5, &w1, 1, newer.clone());
    assert!(replied_ok(&effects, 1));
    assert!(persisted(&effects));
    assert!(pushed_members(&effects));
    assert!(announced_change(&effects));
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq + 1);
    let again = report(&mut hub, &m5, &w1, 2, newer);
    assert!(replied_ok(&again, 2));
    assert!(!persisted(&again));
    assert!(!pushed_members(&again));
}

#[test]
fn the_home_reporting_its_own_unchanged_versions_pushes_nothing() {
    let (mut hub, m5, _) = pair();
    let seq = hub.core(&m5).state().expect("state").seq;
    for _ in 0..3 {
        let effects = hub.local(
            &m5,
            LocalCmd::SetVersions {
                versions: versions("macos"),
            },
        );
        assert!(!persisted(&effects));
        assert!(!pushed_members(&effects));
    }
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq);
    let effects = hub.local(
        &m5,
        LocalCmd::SetVersions {
            versions: versions_updated("macos"),
        },
    );
    assert!(persisted(&effects));
    assert!(pushed_members(&effects));
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq + 1);
}

#[test]
fn only_the_newest_open_tokens_can_still_join() {
    let (mut hub, m5, _) = pair();
    let extra = 4;
    let total = MAX_OPEN_TOKENS + extra;
    for i in 0..total {
        hub.tick(&m5, 1);
        hub.local(
            &m5,
            LocalCmd::IssueToken {
                secret: format!("t{i}"),
                minter: m5.clone(),
            },
        );
    }
    let try_join = |hub: &mut Hub, name: &str, secret: String| {
        let who = hub.add(name, "linux", false);
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
                me: member(name, "linux", false),
            },
        );
        hub.linked.push((m5.clone(), who.clone()));
        hub.run(&m5, Input::Linked { peer: who.clone() });
        hub.run(&who, Input::Linked { peer: m5.clone() });
        hub.seen.iter().any(|(n, e)| {
            n == name
                && matches!(e, Effect::Answer { result: Ok(v), .. } if v == &json!({"state": "pending", "item": format!("node-join:{who}")}))
        })
    };
    assert!(!try_join(&mut hub, "late1", "t0".to_string()));
    assert!(try_join(&mut hub, "late2", format!("t{}", total - 1)));
}
