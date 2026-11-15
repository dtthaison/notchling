use serde_json::{json, Value};

use super::core_tests::{answers, pair, Hub};
use super::{Effect, Input, LocalCmd};
use crate::cluster::failover::Failover;
use crate::cluster::id::NodeId;
use crate::cluster::members::Term;
use crate::cluster::state::Command;
use crate::cluster::testkit::id;
use crate::cluster::wire::Msg;

fn failover_of(hub: &Hub, who: &NodeId) -> Failover {
    hub.core(who).state().expect("state").failover
}

fn status_of(hub: &Hub, who: &NodeId) -> Value {
    hub.core(who).status()["failover"].clone()
}

fn with_client() -> (Hub, NodeId, NodeId) {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    (hub, m5, m1)
}

fn unlink(hub: &mut Hub, a: &NodeId, b: &NodeId) {
    hub.linked
        .retain(|(x, y)| !((x == a && y == b) || (x == b && y == a)));
    hub.run(a, Input::Unlinked { peer: b.clone() });
    hub.run(b, Input::Unlinked { peer: a.clone() });
}

fn arm(hub: &mut Hub, home: &NodeId, node: &NodeId, id: u64) {
    let term: Term = hub.core(home).state().expect("state").term.clone();
    let mut out = Vec::new();
    hub.nodes
        .get_mut(home)
        .expect("home")
        .apply(
            Command::Armed {
                node: node.clone(),
                term,
                id,
            },
            &mut out,
        )
        .expect("armed");
}

fn set(hub: &mut Hub, who: &NodeId, failover: Failover) -> Vec<Effect> {
    hub.local(who, LocalCmd::SetFailover { failover })
}

#[test]
fn the_primary_journals_the_value_and_tells_every_member() {
    let (mut hub, m5, m1) = with_client();
    assert_eq!(failover_of(&hub, &m5), Failover::Off);
    let effects = set(&mut hub, &m5, Failover::After30m);
    assert_eq!(failover_of(&hub, &m5), Failover::After30m);
    assert!(answers(&effects).iter().all(|r| r.is_ok()));
    assert_eq!(failover_of(&hub, &m1), Failover::After30m);
    let journaled = hub.effects_of("m5").into_iter().any(|e| {
        matches!(
            e,
            Effect::Persist { entry, .. }
                if entry.command == Command::SetFailover { failover: Failover::After30m }
        )
    });
    assert!(journaled);
    let pushed = hub.effects_of("m5").into_iter().any(|e| {
        matches!(
            e,
            Effect::Send {
                msg: Msg::Members {
                    failover: Some(Failover::After30m),
                    ..
                },
                ..
            }
        )
    });
    assert!(pushed);
}

#[test]
fn a_client_member_relays_to_the_primary_and_never_journals_it_first() {
    let (mut hub, m5, m1) = with_client();
    let effects = set(&mut hub, &m1, Failover::After1h);
    assert_eq!(failover_of(&hub, &m5), Failover::After1h);
    assert_eq!(failover_of(&hub, &m1), Failover::After1h);
    assert!(answers(&effects).iter().all(|r| r.is_ok()), "{effects:?}");
    let relayed = hub.effects_of("m1").into_iter().any(|e| {
        matches!(
            e,
            Effect::Send { msg: Msg::Call { op, args, .. }, .. }
                if op == "cluster.failover.set" && args == &json!({"failover": "1h"})
        )
    });
    assert!(relayed);
}

#[test]
fn a_worker_that_is_no_client_is_refused_by_the_primary() {
    let (mut hub, m5, w1) = pair();
    set(&mut hub, &w1, Failover::After1h);
    assert_eq!(failover_of(&hub, &m5), Failover::Off);
    assert_eq!(failover_of(&hub, &w1), Failover::Off);
}

#[test]
fn a_member_that_hears_a_value_adopts_it_and_a_late_joiner_starts_with_it() {
    let (mut hub, m5, m1) = with_client();
    set(&mut hub, &m5, Failover::After1h);
    assert_eq!(failover_of(&hub, &m1), Failover::After1h);
    hub.add("w2", "linux", false);
    hub.join("m5", "w2", "linux", false);
    assert_eq!(failover_of(&hub, &id("w2")), Failover::After1h);
    set(&mut hub, &m5, Failover::Off);
    assert_eq!(failover_of(&hub, &id("w2")), Failover::Off);
}

#[test]
fn an_offline_primary_refuses_the_change_and_changes_nothing_here() {
    let (mut hub, m5, m1) = with_client();
    unlink(&mut hub, &m5, &m1);
    let effects = set(&mut hub, &m1, Failover::After30m);
    let refused = answers(&effects)
        .iter()
        .any(|r| r.as_ref().is_err_and(|e| e.message.contains("offline")));
    assert!(refused, "{effects:?}");
    assert_eq!(failover_of(&hub, &m1), Failover::Off);
}

#[test]
fn status_names_the_value_the_target_and_whether_it_is_armed() {
    let (mut hub, m5, m1) = with_client();
    assert_eq!(
        status_of(&hub, &m5),
        json!({"value": "off", "armed": false, "target": id("m1")})
    );
    set(&mut hub, &m5, Failover::After30m);
    assert_eq!(status_of(&hub, &m5)["value"], "30m");
    assert_eq!(status_of(&hub, &m5)["armed"], false);
    assert_eq!(status_of(&hub, &m1)["armed"], false);
    arm(&mut hub, &m5, &m1, 1);
    assert_eq!(status_of(&hub, &m5)["armed"], true);
}

#[test]
fn a_member_hears_that_the_target_is_armed_in_the_next_members_message() {
    let (mut hub, m5, m1) = with_client();
    set(&mut hub, &m5, Failover::After30m);
    arm(&mut hub, &m5, &m1, 1);
    hub.tick_all(0);
    unlink(&mut hub, &m5, &m1);
    hub.link(&m5, &m1);
    hub.tick_all(0);
    assert_eq!(status_of(&hub, &m1)["armed"], true);
    set(&mut hub, &m5, Failover::After1h);
    assert_eq!(status_of(&hub, &m1)["armed"], false);
}

#[test]
fn an_older_members_message_without_the_fields_changes_nothing() {
    let (mut hub, m5, m1) = with_client();
    set(&mut hub, &m5, Failover::After30m);
    let state = hub.core(&m5).state().expect("state").clone();
    let members: Vec<_> = state.members.values().cloned().collect();
    let msg = Msg::Members {
        term: state.term.clone(),
        home: m5.clone(),
        vault: Some(state.vault.clone()),
        members,
        failover: None,
        armed: false,
    };
    hub.run(&m1, Input::Msg { from: m5, msg });
    assert_eq!(failover_of(&hub, &m1), Failover::After30m);
}
