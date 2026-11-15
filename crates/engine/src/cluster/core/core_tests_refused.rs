use serde_json::Value;

use super::core_tests::{pair, Hub};
use super::core_tests_adopt::pair_of_promotables;
use super::{Effect, Input, LocalCmd, REASON_MAX};
use crate::cluster::id::NodeId;
use crate::cluster::join::JoinToken;
use crate::cluster::members::Term;
use crate::cluster::testkit::{fp, id, member};
use crate::cluster::wire::Msg;

fn refused(reason: &str, term: Option<Term>) -> Msg {
    Msg::Refused {
        reason: reason.to_string(),
        home: term.as_ref().map(|t| t.node.clone()),
        term,
    }
}

fn step(hub: &mut Hub, who: &NodeId, from: &NodeId, msg: Msg) -> Vec<Effect> {
    hub.nodes.get_mut(who).expect("node").step(Input::Msg {
        from: from.clone(),
        msg,
    })
}

fn ungreet(hub: &mut Hub, who: &NodeId, peer: &NodeId) {
    let core = hub.nodes.get_mut(who).expect("node");
    core.step(Input::Unlinked { peer: peer.clone() });
    core.step(Input::Linked { peer: peer.clone() });
    assert!(!core.online().contains(peer));
}

fn published(effects: &[Effect]) -> Vec<&Value> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Publish { name, data, .. } if name == "cluster.refused" => Some(data),
            _ => None,
        })
        .collect()
}

fn newer(who: &NodeId) -> Term {
    Term {
        n: 2,
        node: who.clone(),
    }
}

fn joining(hub: &mut Hub, home: &NodeId) -> NodeId {
    let w3 = hub.add("w3", "linux", false);
    let token = JoinToken {
        cluster: "c1".to_string(),
        home: home.clone(),
        fingerprint: fp("m5"),
        addrs: vec![],
        port: 1,
        secret: "s".to_string(),
    };
    hub.local(
        &w3,
        LocalCmd::JoinWith {
            token,
            me: member("w3", "linux", false),
        },
    );
    w3
}

#[test]
fn a_refusal_from_outside_the_cluster_does_nothing() {
    let (mut hub, m5, w1) = pair_of_promotables();
    let nobody = id("nobody");
    for reason in ["stale_term", "removed", "anything"] {
        let effects = step(&mut hub, &m5, &nobody, refused(reason, Some(newer(&w1))));
        assert!(effects.is_empty(), "{reason}: {effects:?}");
    }
    let state = hub.core(&m5).state().expect("state");
    assert_eq!(state.home, m5);
    assert_eq!(state.term.n, 1);
}

#[test]
fn a_refusal_from_a_removed_member_does_nothing() {
    let (mut hub, m5, w1) = pair();
    hub.local(&m5, LocalCmd::Remove { node: w1.clone() });
    let effects = step(&mut hub, &m5, &w1, refused("stale_term", Some(newer(&w1))));
    assert!(published(&effects).is_empty());
    assert!(!effects
        .iter()
        .any(|e| matches!(e, Effect::SteppedDown { .. })));
}

#[test]
fn a_workers_refusal_is_published_but_cannot_move_the_term() {
    let (mut hub, m5, w1) = pair();
    let effects = step(&mut hub, &m5, &w1, refused("stale_term", Some(newer(&w1))));
    assert_eq!(published(&effects).len(), 1);
    assert!(!effects
        .iter()
        .any(|e| matches!(e, Effect::SteppedDown { .. })));
    assert_eq!(hub.core(&m5).state().expect("state").home, m5);
}

#[test]
fn a_stale_term_refusal_before_the_greeting_still_steps_the_old_primary_down() {
    let (mut hub, m5, w1) = pair_of_promotables();
    ungreet(&mut hub, &m5, &w1);
    let effects = step(&mut hub, &m5, &w1, refused("stale_term", Some(newer(&w1))));
    assert!(effects
        .iter()
        .any(|e| matches!(e, Effect::SteppedDown { home, .. } if *home == w1)));
    assert_eq!(hub.core(&m5).state().expect("state").home, w1);
    assert_eq!(published(&effects).len(), 1);
}

#[test]
fn removed_by_the_home_before_the_greeting_still_wipes() {
    let (mut hub, m5, w1) = pair();
    ungreet(&mut hub, &w1, &m5);
    let effects = step(&mut hub, &w1, &m5, refused("removed", None));
    assert!(effects.iter().any(|e| matches!(e, Effect::Wipe)));
    assert!(hub.core(&w1).state().is_none());
}

#[test]
fn removed_from_a_member_that_is_not_the_home_does_not_wipe() {
    let (mut hub, m5, w1) = pair_of_promotables();
    let effects = step(&mut hub, &m5, &w1, refused("removed", None));
    assert!(!effects.iter().any(|e| matches!(e, Effect::Wipe)));
    assert!(hub.core(&m5).state().is_some());
    assert_eq!(published(&effects).len(), 1);
}

#[test]
fn a_refusal_reason_is_capped_and_its_control_characters_replaced() {
    let (mut hub, m5, w1) = pair();
    let huge = "é".repeat(1 << 20);
    let effects = step(&mut hub, &m5, &w1, refused(&huge, None));
    let data = published(&effects);
    assert_eq!(data.len(), 1);
    let reason = data[0]["reason"].as_str().expect("reason");
    assert_eq!(reason.chars().count(), REASON_MAX);
    assert!(reason.chars().all(|c| c == 'é'));

    let nasty = "\u{1b}[31mred\nline\u{0}\u{7f}";
    let effects = step(&mut hub, &m5, &w1, refused(nasty, None));
    let reason = published(&effects)[0]["reason"]
        .as_str()
        .expect("reason")
        .to_string();
    assert_eq!(reason, " [31mred line  ");
    assert!(!reason.chars().any(char::is_control));
}

#[test]
fn a_join_refusal_is_cleaned_before_it_is_shown_or_published() {
    let (mut hub, m5, _) = pair();
    let w3 = joining(&mut hub, &m5);
    let reason = format!("\u{1b}[2J{}", "x".repeat(1 << 20));
    let effects = step(&mut hub, &w3, &m5, refused(&reason, None));
    let shown = effects
        .iter()
        .find_map(|e| match e {
            Effect::Answer {
                result: Err(err), ..
            } => Some(err.message.clone()),
            _ => None,
        })
        .expect("the joiner hears it");
    assert_eq!(shown.chars().count(), REASON_MAX);
    assert!(shown.starts_with(" [2Jxx"));
    let data = published(&effects);
    assert_eq!(data[0]["reason"].as_str().expect("reason"), shown);
}

#[test]
fn a_stranger_cannot_end_a_join_in_progress() {
    let (mut hub, m5, _) = pair();
    let w3 = joining(&mut hub, &m5);
    let effects = step(&mut hub, &w3, &id("nobody"), refused("bad token", None));
    assert!(effects.is_empty());
    let again = step(&mut hub, &w3, &m5, refused("bad token", None));
    assert!(again
        .iter()
        .any(|e| matches!(e, Effect::Answer { result: Err(_), .. })));
}
