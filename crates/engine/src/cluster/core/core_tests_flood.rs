use serde_json::{json, Value};

use super::core_tests::{pair, Hub};
use super::core_tests_adopt::pair_of_promotables;
use super::{Effect, Input, LocalCmd, VERSIONS_GAP_MS};
use crate::cluster::id::NodeId;
use crate::cluster::join::{JoinToken, MAX_OPEN_TOKENS, MAX_TOKENS_PER_MINTER};
use crate::cluster::testkit::{fp, member, versions, versions_updated};
use crate::cluster::versions::Versions;
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

fn hello_with(hub: &mut Hub, home: &NodeId, from: &NodeId, versions: Versions) -> Vec<Effect> {
    let term = hub.core(home).highest_term().cloned().expect("term");
    hub.nodes.get_mut(home).expect("node").step(Input::Msg {
        from: from.clone(),
        msg: Msg::Hello {
            node: from.clone(),
            cluster: Some("c1".to_string()),
            versions,
            boot_id: "boot".to_string(),
            term: Some(term),
        },
    })
}

fn members_sent_to(effects: &[Effect], to: &NodeId) -> bool {
    effects.iter().any(|e| {
        matches!(
            e,
            Effect::Send { to: peer, msg: Msg::Members { .. } } if peer == to
        )
    })
}

fn trio() -> (Hub, NodeId, NodeId, NodeId) {
    let (mut hub, m5, w1) = pair();
    let w2 = hub.add("w2", "linux", false);
    hub.join("m5", "w2", "linux", false);
    (hub, m5, w1, w2)
}

fn stored(hub: &Hub, home: &NodeId, who: &NodeId) -> Versions {
    hub.core(home).state().expect("state").members[who]
        .versions
        .clone()
        .expect("reported")
}

fn value(v: &Versions) -> Value {
    serde_json::to_value(v).expect("versions")
}

#[test]
fn a_member_flapping_between_two_reports_is_journaled_once_per_gap() {
    let (mut hub, m5, w1) = pair();
    let (a, b) = (versions_updated("linux"), versions("linux"));
    let seq = hub.core(&m5).state().expect("state").seq;
    let mut persisted_count = 0;
    for id in 1..=40u64 {
        let report_now = if id % 2 == 1 { &a } else { &b };
        let effects = report(&mut hub, &m5, &w1, id, value(report_now));
        assert!(replied_ok(&effects, id), "{effects:?}");
        persisted_count += usize::from(persisted(&effects));
    }
    assert_eq!(persisted_count, 1);
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq + 1);
    assert_eq!(stored(&hub, &m5, &w1), a);
    let effects = hub.tick(&m5, VERSIONS_GAP_MS);
    assert!(persisted(&effects), "the latest report is recorded");
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq + 2);
    assert_eq!(stored(&hub, &m5, &w1), b);
    let quiet = hub.tick(&m5, VERSIONS_GAP_MS);
    assert!(!persisted(&quiet));
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq + 2);
}

#[test]
fn a_held_report_is_pushed_to_every_member_when_it_is_recorded() {
    let (mut hub, m5, w1, w2) = trio();
    report(&mut hub, &m5, &w1, 1, value(&versions_updated("linux")));
    report(&mut hub, &m5, &w1, 2, value(&versions("linux")));
    let held = hub.tick(&m5, 1);
    assert!(!persisted(&held));
    assert!(!members_sent_to(&held, &w2));
    let due = hub.tick(&m5, VERSIONS_GAP_MS);
    assert!(persisted(&due));
    assert!(members_sent_to(&due, &w1) && members_sent_to(&due, &w2));
    assert!(due
        .iter()
        .any(|e| matches!(e, Effect::Publish { name, .. } if name == "cluster.changed")));
}

#[test]
fn a_held_report_that_ends_up_equal_to_the_stored_one_is_never_journaled() {
    let (mut hub, m5, w1) = pair();
    let seq = hub.core(&m5).state().expect("state").seq;
    report(&mut hub, &m5, &w1, 1, value(&versions_updated("linux")));
    report(&mut hub, &m5, &w1, 2, value(&versions("linux")));
    report(&mut hub, &m5, &w1, 3, value(&versions_updated("linux")));
    let due = hub.tick(&m5, VERSIONS_GAP_MS);
    assert!(!persisted(&due));
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq + 1);
}

#[test]
fn a_change_after_a_quiet_gap_is_journaled_at_once() {
    let (mut hub, m5, w1) = pair();
    let seq = hub.core(&m5).state().expect("state").seq;
    report(&mut hub, &m5, &w1, 1, value(&versions_updated("linux")));
    hub.tick(&m5, VERSIONS_GAP_MS);
    let effects = report(&mut hub, &m5, &w1, 2, value(&versions("linux")));
    assert!(persisted(&effects));
    assert!(pushed_members(&effects));
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq + 2);
}

#[test]
fn each_member_has_its_own_gap() {
    let (mut hub, m5, w1, w2) = trio();
    let seq = hub.core(&m5).state().expect("state").seq;
    assert!(persisted(&report(
        &mut hub,
        &m5,
        &w1,
        1,
        value(&versions_updated("linux"))
    )));
    assert!(persisted(&report(
        &mut hub,
        &m5,
        &w2,
        2,
        value(&versions_updated("linux"))
    )));
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq + 2);
}

#[test]
fn hellos_alternating_between_two_reports_are_journaled_once_per_gap() {
    let (mut hub, m5, w1, w2) = trio();
    let (a, b) = (versions_updated("linux"), versions("linux"));
    let seq = hub.core(&m5).state().expect("state").seq;
    let mut persisted_count = 0;
    let mut broadcasts = 0;
    for n in 0..40 {
        let report_now = if n % 2 == 0 { &a } else { &b };
        let effects = hello_with(&mut hub, &m5, &w1, report_now.clone());
        persisted_count += usize::from(persisted(&effects));
        broadcasts += usize::from(members_sent_to(&effects, &w2));
    }
    assert_eq!(persisted_count, 1);
    assert_eq!(broadcasts, 1, "only the first change reaches the others");
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq + 1);
    assert_eq!(stored(&hub, &m5, &w1), a);
    let due = hub.tick(&m5, VERSIONS_GAP_MS);
    assert!(persisted(&due));
    assert!(members_sent_to(&due, &w2));
    assert_eq!(stored(&hub, &m5, &w1), b);
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq + 2);
}

#[test]
fn a_hello_repeating_the_stored_versions_is_not_journaled_or_broadcast() {
    let (mut hub, m5, w1, w2) = trio();
    let same = versions("linux");
    let seq = hub.core(&m5).state().expect("state").seq;
    for _ in 0..5 {
        let effects = hello_with(&mut hub, &m5, &w1, same.clone());
        assert!(!persisted(&effects));
        assert!(members_sent_to(&effects, &w1));
        assert!(!members_sent_to(&effects, &w2));
    }
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq);
}

#[test]
fn hello_and_the_versions_call_share_one_gap() {
    let (mut hub, m5, w1) = pair();
    let seq = hub.core(&m5).state().expect("state").seq;
    assert!(persisted(&hello_with(
        &mut hub,
        &m5,
        &w1,
        versions_updated("linux")
    )));
    let effects = report(&mut hub, &m5, &w1, 1, value(&versions("linux")));
    assert!(replied_ok(&effects, 1));
    assert!(!persisted(&effects));
    let held = hello_with(&mut hub, &m5, &w1, versions_updated("linux"));
    assert!(!persisted(&held));
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq + 1);
}

#[test]
fn a_held_report_is_dropped_when_the_home_steps_down() {
    let (mut hub, m5, w1) = pair_of_promotables();
    report(&mut hub, &m5, &w1, 1, value(&versions_updated("macos")));
    report(&mut hub, &m5, &w1, 2, value(&versions("macos")));
    hub.nodes.get_mut(&w1).expect("node").bump_term();
    let term = hub.core(&w1).highest_term().cloned().expect("term");
    hub.nodes.get_mut(&m5).expect("node").step(Input::Msg {
        from: w1.clone(),
        msg: Msg::HelloOk {
            node: w1.clone(),
            cluster: "c1".to_string(),
            home: term.node.clone(),
            term,
        },
    });
    assert!(!hub.core(&m5).is_home());
    let seq = hub.core(&m5).state().expect("state").seq;
    let due = hub.tick(&m5, VERSIONS_GAP_MS);
    assert!(!persisted(&due));
    assert_eq!(hub.core(&m5).state().expect("state").seq, seq);
}

fn strangers_connect_and_drop(hub: &mut Hub, home: &NodeId, count: usize) {
    for n in 0..count {
        let stranger = crate::cluster::testkit::id(&format!("stranger-{n}"));
        hub.nodes.get_mut(home).expect("node").step(Input::Linked {
            peer: stranger.clone(),
        });
        hub.nodes
            .get_mut(home)
            .expect("node")
            .step(Input::Unlinked { peer: stranger });
    }
}

#[test]
fn strangers_connecting_and_dropping_leave_nothing_behind() {
    let (mut hub, m5, _) = pair();
    let before = hub.core(&m5).peers.len();
    strangers_connect_and_drop(&mut hub, &m5, 2_000);
    assert_eq!(hub.core(&m5).peers.len(), before);
}

#[test]
fn a_stranger_the_node_closes_on_leaves_nothing_behind() {
    let (mut hub, m5, _) = pair();
    let before = hub.core(&m5).peers.len();
    for n in 0..200 {
        let stranger = crate::cluster::testkit::id(&format!("greeter-{n}"));
        let node = hub.nodes.get_mut(&m5).expect("node");
        node.step(Input::Linked {
            peer: stranger.clone(),
        });
        let effects = node.step(Input::Msg {
            from: stranger.clone(),
            msg: Msg::Hello {
                node: stranger.clone(),
                cluster: Some("c1".to_string()),
                versions: versions("linux"),
                boot_id: "boot".to_string(),
                term: None,
            },
        });
        assert!(effects
            .iter()
            .any(|e| matches!(e, Effect::Close { peer } if *peer == stranger)));
    }
    assert_eq!(hub.core(&m5).peers.len(), before);
}

#[test]
fn a_stranger_that_stays_linked_keeps_its_entry_until_it_drops() {
    let (mut hub, m5, _) = pair();
    let before = hub.core(&m5).peers.len();
    let stranger = crate::cluster::testkit::id("stays");
    let node = hub.nodes.get_mut(&m5).expect("node");
    node.step(Input::Linked {
        peer: stranger.clone(),
    });
    assert_eq!(node.peers.len(), before + 1);
    node.step(Input::Unlinked { peer: stranger });
    assert_eq!(node.peers.len(), before);
}

#[test]
fn a_member_that_drops_keeps_its_entry() {
    let (mut hub, m5, w1) = pair();
    let before = hub.core(&m5).peers.len();
    hub.run(&m5, Input::Unlinked { peer: w1.clone() });
    assert_eq!(hub.core(&m5).peers.len(), before);
    assert!(!hub.core(&m5).online().contains(&w1));
}

#[test]
fn strangers_dropping_do_not_disturb_a_join_in_progress() {
    let (mut hub, m5, _) = pair();
    let w3 = hub.add("w3", "linux", false);
    hub.local(
        &m5,
        LocalCmd::IssueToken {
            secret: "s3".to_string(),
            minter: m5.clone(),
        },
    );
    let token = JoinToken {
        cluster: "c1".to_string(),
        home: m5.clone(),
        fingerprint: fp("m5"),
        addrs: vec!["100.64.0.5".to_string()],
        port: 47823,
        secret: "s3".to_string(),
    };
    hub.local(
        &w3,
        LocalCmd::JoinWith {
            token,
            me: member("w3", "linux", false),
        },
    );
    hub.linked.push((m5.clone(), w3.clone()));
    hub.run(&m5, Input::Linked { peer: w3.clone() });
    hub.run(&w3, Input::Linked { peer: m5.clone() });
    assert!(hub.core(&m5).peers.contains_key(&w3));
    strangers_connect_and_drop(&mut hub, &m5, 300);
    hub.linked.retain(|(a, b)| !(a == &m5 && b == &w3));
    hub.run(&m5, Input::Unlinked { peer: w3.clone() });
    hub.run(&w3, Input::Unlinked { peer: m5.clone() });
    assert!(
        hub.core(&m5).peers.contains_key(&w3),
        "a pending member keeps its entry"
    );
    hub.linked.push((m5.clone(), w3.clone()));
    hub.run(&m5, Input::Linked { peer: w3.clone() });
    hub.run(&w3, Input::Linked { peer: m5.clone() });
    hub.local(
        &m5,
        LocalCmd::Approve {
            node: w3.clone(),
            client: false,
            can_be_master: false,
        },
    );
    hub.tick_all(0);
    let joined = hub.core(&w3).state().expect("the joiner is in the cluster");
    assert_eq!(joined.home, m5);
    assert!(hub.core(&m5).online().contains(&w3));
    assert!(hub.core(&w3).online().contains(&m5));
}

#[test]
fn a_denied_joiner_leaves_nothing_behind_once_it_drops() {
    let (mut hub, m5, _) = pair();
    let before = hub.core(&m5).peers.len();
    let w3 = hub.add("w3", "linux", false);
    hub.local(
        &m5,
        LocalCmd::IssueToken {
            secret: "s3".to_string(),
            minter: m5.clone(),
        },
    );
    let token = JoinToken {
        cluster: "c1".to_string(),
        home: m5.clone(),
        fingerprint: fp("m5"),
        addrs: vec![],
        port: 1,
        secret: "s3".to_string(),
    };
    hub.local(
        &w3,
        LocalCmd::JoinWith {
            token,
            me: member("w3", "linux", false),
        },
    );
    hub.linked.push((m5.clone(), w3.clone()));
    hub.run(&m5, Input::Linked { peer: w3.clone() });
    hub.run(&w3, Input::Linked { peer: m5.clone() });
    hub.local(&m5, LocalCmd::Deny { node: w3.clone() });
    hub.run(&m5, Input::Unlinked { peer: w3.clone() });
    assert_eq!(hub.core(&m5).peers.len(), before);
}
