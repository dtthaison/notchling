use serde_json::{json, Value};

use super::core_tests::{pair, Hub};
use super::{Effect, Input, LocalCmd};
use crate::cluster::id::NodeId;
use crate::cluster::join::{JoinToken, MAX_OPEN_TOKENS, MAX_TOKENS_PER_MINTER};
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

fn joins_with(hub: &mut Hub, home: &NodeId, name: &str, secret: String) -> bool {
    let who = hub.add(name, "linux", false);
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
            me: member(name, "linux", false),
        },
    );
    hub.linked.push((home.clone(), who.clone()));
    hub.run(home, Input::Linked { peer: who.clone() });
    hub.run(&who, Input::Linked { peer: home.clone() });
    hub.seen.iter().any(|(n, e)| {
        n == name
            && matches!(e, Effect::Answer { result: Ok(v), .. } if v == &json!({"state": "pending", "item": format!("node-join:{who}")}))
    })
}

fn issue(hub: &mut Hub, home: &NodeId, minter: &NodeId, secret: String) {
    hub.tick(home, 1);
    hub.local(
        home,
        LocalCmd::IssueToken {
            secret,
            minter: minter.clone(),
        },
    );
}

#[test]
fn only_the_newest_open_tokens_can_still_join() {
    let (mut hub, m5, _) = pair();
    let extra = 4;
    let total = MAX_OPEN_TOKENS + extra;
    for i in 0..total {
        issue(&mut hub, &m5, &m5, format!("t{i}"));
    }
    assert!(!joins_with(&mut hub, &m5, "late1", "t0".to_string()));
    assert!(joins_with(
        &mut hub,
        &m5,
        "late2",
        format!("t{}", total - 1)
    ));
}

#[test]
fn a_workers_tokens_cannot_push_out_the_primarys_pending_one() {
    let (mut hub, m5, w1) = pair();
    issue(&mut hub, &m5, &m5, "home".to_string());
    for i in 0..20 {
        issue(&mut hub, &m5, &w1, format!("w{i}"));
    }
    assert!(!joins_with(&mut hub, &m5, "late1", "w0".to_string()));
    assert!(!joins_with(&mut hub, &m5, "late2", "w15".to_string()));
    assert!(joins_with(&mut hub, &m5, "late3", "w16".to_string()));
    assert!(joins_with(&mut hub, &m5, "late4", "home".to_string()));
}

#[test]
fn the_primary_making_more_than_its_share_loses_its_own_oldest() {
    let (mut hub, m5, _) = pair();
    let made = MAX_TOKENS_PER_MINTER + 1;
    for i in 0..made {
        issue(&mut hub, &m5, &m5, format!("h{i}"));
    }
    assert!(!joins_with(&mut hub, &m5, "late0", "h0".to_string()));
    for i in 1..made {
        assert!(joins_with(
            &mut hub,
            &m5,
            &format!("late{i}"),
            format!("h{i}")
        ));
    }
}

#[test]
fn a_full_table_of_member_tokens_still_keeps_the_primarys() {
    let (mut hub, m5, w1) = pair();
    let w2 = hub.add("w2", "linux", false);
    let w3 = hub.add("w3", "linux", false);
    let w4 = hub.add("w4", "linux", false);
    hub.join("m5", "w2", "linux", false);
    hub.join("m5", "w3", "linux", false);
    hub.join("m5", "w4", "linux", false);
    issue(&mut hub, &m5, &m5, "home".to_string());
    for round in 0..MAX_TOKENS_PER_MINTER {
        for (n, who) in [&w1, &w2, &w3, &w4].into_iter().enumerate() {
            issue(&mut hub, &m5, who, format!("w{n}-{round}"));
        }
    }
    assert!(!joins_with(&mut hub, &m5, "late1", "w0-0".to_string()));
    assert!(joins_with(&mut hub, &m5, "late2", "w1-0".to_string()));
    assert!(joins_with(&mut hub, &m5, "late3", "home".to_string()));
}
