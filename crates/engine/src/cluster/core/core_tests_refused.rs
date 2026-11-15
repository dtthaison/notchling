use serde_json::{json, Value};

use super::core_tests::{answers, dispatch_cmd, pair, Hub};
use super::core_tests_adopt::pair_of_promotables;
use super::link::clean_reason;
use super::{Effect, Input, LocalCmd, REASON_MAX};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::job_id;
use crate::cluster::join::JoinToken;
use crate::cluster::members::Term;
use crate::cluster::testkit::{fp, id, member};
use crate::cluster::wire::{Msg, Origin, OriginTier, WireError};

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

#[test]
fn a_reason_loses_its_bidi_and_zero_width_characters() {
    let (mut hub, m5, w1) = pair();
    let effects = step(&mut hub, &m5, &w1, refused("\u{202e}removed", None));
    let reason = published(&effects)[0]["reason"].as_str().expect("reason");
    assert_eq!(reason, " removed");
    let effects = step(&mut hub, &m5, &w1, refused("pass\u{200b}word", None));
    let reason = published(&effects)[0]["reason"].as_str().expect("reason");
    assert_eq!(reason, "pass word");
}

#[test]
fn every_listed_format_character_is_replaced_and_its_neighbours_are_not() {
    let ranges = [
        (0xad, 0xad),
        (0x61c, 0x61c),
        (0x180e, 0x180e),
        (0x200b, 0x200f),
        (0x202a, 0x202e),
        (0x2060, 0x2064),
        (0x2066, 0x206f),
        (0xfeff, 0xfeff),
        (0xfff9, 0xfffb),
    ];
    let listed = |cp: u32| ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(&cp));
    for (lo, hi) in ranges {
        for cp in lo..=hi {
            let c = char::from_u32(cp).expect("scalar");
            assert_eq!(clean_reason(&format!("a{c}b")), "a b", "{cp:x}");
        }
        for cp in [lo - 1, hi + 1] {
            if listed(cp) {
                continue;
            }
            let c = char::from_u32(cp).expect("scalar");
            let text = format!("a{c}b");
            assert_eq!(clean_reason(&text), text, "{cp:x}");
        }
    }
}

fn the_call_out(effects: &[Effect]) -> u64 {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Send {
                msg: Msg::Call { id, .. },
                ..
            } => Some(*id),
            _ => None,
        })
        .expect("a call went out")
}

fn nasty() -> String {
    format!("\u{202e}evil\u{1b}[2J\u{200b}\n{}", "x".repeat(1 << 20))
}

fn is_clean(text: &str) -> bool {
    text.chars().count() <= REASON_MAX
        && !text.chars().any(char::is_control)
        && !text.contains(['\u{202e}', '\u{200b}'])
}

#[test]
fn a_peers_reply_error_is_cleaned_and_capped_before_the_local_caller_sees_it() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.local(
        &m5,
        LocalCmd::Forward {
            node: w1.clone(),
            op: "node.load".to_string(),
            args: json!({}),
            origin: Origin {
                node: m5.clone(),
                tier: OriginTier::Node,
            },
        },
    );
    let call = the_call_out(&effects);
    let err = WireError::new("\u{202e}failed", nasty());
    let reply = Msg::Reply {
        id: call,
        ok: None,
        err: Some(err),
    };
    let effects = step(&mut hub, &m5, &w1, reply);
    let shown = answers(&effects);
    let Some(Err(shown)) = shown.first() else {
        panic!("the caller hears the failure: {effects:?}");
    };
    assert_eq!(shown.code, " failed");
    assert_eq!(shown.message.chars().count(), REASON_MAX);
    assert!(shown.message.starts_with(" evil [2J  xx"));
    assert!(is_clean(&shown.message));
}

#[test]
fn a_stale_term_reply_is_still_read_before_its_text_is_cleaned() {
    let (mut hub, m5, w1) = pair_of_promotables();
    let effects = hub.local(
        &m5,
        LocalCmd::Forward {
            node: w1.clone(),
            op: "node.load".to_string(),
            args: json!({}),
            origin: Origin {
                node: m5.clone(),
                tier: OriginTier::Node,
            },
        },
    );
    let call = the_call_out(&effects);
    let term = newer(&w1);
    let body = json!({"term": term, "home": w1, "pad": "x".repeat(1 << 10)}).to_string();
    let reply = Msg::Reply {
        id: call,
        ok: None,
        err: Some(WireError::new("stale_term", body)),
    };
    let effects = step(&mut hub, &m5, &w1, reply);
    assert_eq!(hub.core(&m5).state().expect("state").term, term);
    let shown = answers(&effects);
    let Some(Err(shown)) = shown.first() else {
        panic!("the caller hears the failure: {effects:?}");
    };
    assert_eq!(shown.code, "stale_term");
    assert_eq!(shown.message.chars().count(), REASON_MAX);
}

#[test]
fn a_peers_failure_text_is_cleaned_before_it_becomes_a_jobs_detail() {
    let (mut hub, m5, w1) = pair();
    let started = hub.local(&m5, dispatch_cmd("k", "w1", Tool::Shell));
    let call = the_call_out(&started);
    let reply = Msg::Reply {
        id: call,
        ok: None,
        err: Some(WireError::new("failed", nasty())),
    };
    let effects = step(&mut hub, &m5, &w1, reply);
    let detail = hub.core(&m5).state().expect("state").jobs[&job_id("k")]
        .detail
        .clone()
        .expect("the failure is recorded");
    assert!(is_clean(&detail), "{detail}");
    let shown = answers(&effects);
    let Some(Err(shown)) = shown.first() else {
        panic!("the caller hears the failure: {effects:?}");
    };
    assert!(is_clean(&shown.message));
}

#[test]
fn an_op_a_peer_names_is_cleaned_before_it_is_published() {
    let (mut hub, m5, w1) = pair();
    let term = hub.core(&m5).state().expect("state").term.clone();
    for op in [
        format!("node.\u{202e}evil\n{}", "y".repeat(1 << 16)),
        "\u{202e}x".to_string(),
    ] {
        let call = Msg::Call {
            id: 5,
            op,
            args: json!({}),
            origin: Origin {
                node: w1.clone(),
                tier: OriginTier::Node,
            },
            term: term.clone(),
        };
        let effects = step(&mut hub, &m5, &w1, call);
        let data = published(&effects);
        assert_eq!(data.len(), 1, "{effects:?}");
        let op = data[0]["op"].as_str().expect("op");
        assert!(is_clean(op), "{op}");
    }
}
