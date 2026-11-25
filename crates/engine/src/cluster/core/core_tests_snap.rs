use std::collections::BTreeSet;

use serde_json::json;

use super::core_tests::{answers, Hub};
use super::{Effect, Input, LocalCmd, SnapAsk};
use crate::cluster::id::NodeId;
use crate::cluster::snapshot::{Observed, SessionState, Trigger};
use crate::cluster::testkit::id;
use crate::cluster::wire::{Msg, Origin, OriginTier};
use crate::sessions::{Agent, AgentGroup, AgentTarget};

const NOW: i64 = 5_000_000;

fn agent(name: &str) -> Agent {
    Agent {
        id: name.to_string(),
        name: name.to_string(),
        project: "repo".to_string(),
        cwd: "/work/repo".to_string(),
        group: AgentGroup::Completed,
        label: "Done".to_string(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job("ab12cd34".to_string()),
        suggested_reply: None,
        transcript_path: Some(format!("/t/{name}.jsonl")),
        host: "Background".to_string(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: None,
    }
}

fn with_standby() -> Hub {
    let mut hub = Hub::new();
    hub.add("m5", "macos", true);
    hub.add("m1", "macos", true);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.local(
        &id("m5"),
        LocalCmd::MainSeen {
            agents: vec![agent("sess-1")],
            codex_exec: BTreeSet::new(),
            respawn: Default::default(),
        },
    );
    hub
}

fn observed(state: SessionState, at: i64) -> Observed {
    Observed {
        now_ms: at,
        session_state: [("sess-1".to_string(), state)].into(),
        ..Observed::default()
    }
}

fn snapshots(effects: &[Effect]) -> Vec<&SnapAsk> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Snapshot { plan } => Some(plan),
            _ => None,
        })
        .collect()
}

fn observe(hub: &mut Hub, who: &str, observed: Observed) -> Vec<Effect> {
    hub.run(&id(who), Input::Observed { observed })
}

#[test]
fn a_turn_end_asks_for_a_snapshot_for_every_can_be_master_member() {
    let mut hub = with_standby();
    assert!(snapshots(&observe(
        &mut hub,
        "m5",
        observed(SessionState::Working, NOW)
    ))
    .is_empty());
    let effects = observe(&mut hub, "m5", observed(SessionState::Idle, NOW + 2_000));
    let asks = snapshots(&effects);
    assert_eq!(asks.len(), 1);
    let ask = asks[0];
    assert_eq!(ask.trigger, Trigger::TurnEnd("sess-1".to_string()));
    assert_eq!(ask.targets, vec![id("m1")]);
    assert_eq!(ask.home, id("m5"));
    assert_eq!(ask.main.len(), 1);
    assert!(ask.seq >= (NOW + 2_000) as u64);
}

#[test]
fn a_set_is_not_asked_again_before_the_timer() {
    let mut hub = with_standby();
    observe(&mut hub, "m5", observed(SessionState::Working, NOW));
    let first = observe(&mut hub, "m5", observed(SessionState::Idle, NOW + 2_000));
    assert_eq!(snapshots(&first).len(), 1);
    let again = observe(&mut hub, "m5", observed(SessionState::Idle, NOW + 4_000));
    assert!(snapshots(&again).is_empty());
    let timer = observe(&mut hub, "m5", observed(SessionState::Idle, NOW + 400_000));
    let asks = snapshots(&timer);
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0].trigger, Trigger::Timer);
    let seqs: Vec<u64> = [&first, &timer]
        .iter()
        .flat_map(|e| snapshots(e).into_iter().map(|a| a.seq))
        .collect();
    assert!(seqs[0] < seqs[1]);
}

#[test]
fn a_turn_end_blocked_by_a_recent_write_waits_for_a_quiet_moment() {
    let mut hub = with_standby();
    observe(&mut hub, "m5", observed(SessionState::Working, NOW));
    let mut busy = observed(SessionState::Idle, NOW + 2_000);
    busy.last_write_ms.insert("sess-1".to_string(), NOW + 1_500);
    assert!(snapshots(&observe(&mut hub, "m5", busy)).is_empty());
    let quiet = observe(&mut hub, "m5", observed(SessionState::Idle, NOW + 20_000));
    let asks = snapshots(&quiet);
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0].trigger, Trigger::TurnEnd("sess-1".to_string()));
}

#[test]
fn nothing_is_asked_without_a_can_be_master_standby() {
    let mut hub = Hub::new();
    hub.add("m5", "macos", true);
    hub.add("c1", "macos", true);
    hub.create("m5");
    hub.join_as("m5", "c1", "macos", true, false);
    observe(&mut hub, "m5", observed(SessionState::Working, NOW));
    let effects = observe(&mut hub, "m5", observed(SessionState::Idle, NOW + 2_000));
    assert!(snapshots(&effects).is_empty());
}

#[test]
fn a_member_that_is_not_the_primary_never_asks() {
    let mut hub = with_standby();
    observe(&mut hub, "m1", observed(SessionState::Working, NOW));
    let effects = observe(&mut hub, "m1", observed(SessionState::Idle, NOW + 2_000));
    assert!(snapshots(&effects).is_empty());
}

#[test]
fn sessions_that_are_not_main_are_left_out() {
    let mut hub = with_standby();
    let mut other = observed(SessionState::Working, NOW);
    other
        .session_state
        .insert("not-main".to_string(), SessionState::Working);
    observe(&mut hub, "m5", other);
    let mut next = observed(SessionState::Working, NOW + 2_000);
    next.session_state
        .insert("not-main".to_string(), SessionState::Idle);
    assert!(snapshots(&observe(&mut hub, "m5", next)).is_empty());
}

#[test]
fn a_snapshot_command_asks_at_once_with_the_given_trigger() {
    let mut hub = with_standby();
    let effects = hub.local(
        &id("m5"),
        LocalCmd::Snapshot {
            trigger: Trigger::Ready(id("m1")),
        },
    );
    let asks = snapshots(&effects);
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0].trigger, Trigger::Ready(id("m1")));
    let answered = answers(&effects);
    let reply = answered[0].as_ref().expect("an answer");
    assert_eq!(reply["asked"], json!(asks[0].seq));
    assert!(reply.get("set").is_none());
    let refused = hub.local(
        &id("m1"),
        LocalCmd::Snapshot {
            trigger: Trigger::Final,
        },
    );
    assert!(snapshots(&refused).is_empty());
}

fn ack_from(hub: &mut Hub, who: &str, set: &str, length: u64) {
    let home = id("m5");
    let term = hub.core(&home).state().expect("state").term.clone();
    hub.run(
        &home,
        Input::Msg {
            from: id(who),
            msg: Msg::Call {
                id: length + 100,
                op: "snap.ack".to_string(),
                args: json!({"set": set, "lengths": {"/t/sess-1.jsonl": length}}).into(),
                origin: Origin {
                    node: id(who),
                    tier: OriginTier::Node,
                },
                term,
            },
        },
    );
}

fn acked_len(hub: &Hub, who: &NodeId) -> Option<u64> {
    hub.core(&id("m5")).acked(who).and_then(|a| {
        a.lengths
            .get(std::path::Path::new("/t/sess-1.jsonl"))
            .copied()
    })
}

#[test]
fn an_ack_from_a_standby_is_kept_per_target_and_never_goes_back() {
    let mut hub = with_standby();
    hub.add("m2", "macos", true);
    hub.join_as("m5", "m2", "macos", true, true);
    ack_from(&mut hub, "m1", "s-20", 200);
    ack_from(&mut hub, "m2", "s-10", 100);
    assert_eq!(acked_len(&hub, &id("m1")), Some(200));
    assert_eq!(acked_len(&hub, &id("m2")), Some(100));
    ack_from(&mut hub, "m1", "s-5", 50);
    assert_eq!(acked_len(&hub, &id("m1")), Some(200));
    let effects = hub.local(
        &id("m5"),
        LocalCmd::Snapshot {
            trigger: Trigger::Final,
        },
    );
    let ask = snapshots(&effects)[0].clone();
    assert_eq!(
        ask.acked.get(std::path::Path::new("/t/sess-1.jsonl")),
        Some(&100)
    );
}

#[test]
fn an_ack_with_a_relative_path_is_ignored() {
    let mut hub = with_standby();
    let home = id("m5");
    let term = hub.core(&home).state().expect("state").term.clone();
    hub.run(
        &home,
        Input::Msg {
            from: id("m1"),
            msg: Msg::Call {
                id: 1,
                op: "snap.ack".to_string(),
                args: json!({"set": "s-1", "lengths": {"t/sess-1.jsonl": 5}}).into(),
                origin: Origin {
                    node: id("m1"),
                    tier: OriginTier::Node,
                },
                term,
            },
        },
    );
    assert!(hub.core(&home).acked(&id("m1")).is_none());
}

#[test]
fn r2_a_fenced_primary_asks_only_for_the_final_set_then_catches_up() {
    let mut hub = with_standby();
    let m5 = id("m5");
    observe(&mut hub, "m5", observed(SessionState::Working, NOW));
    hub.local(
        &m5,
        LocalCmd::Fence {
            reason: Some("The primary is moving to m1".to_string()),
        },
    );
    let turn_end = observe(&mut hub, "m5", observed(SessionState::Idle, NOW + 2_000));
    assert!(snapshots(&turn_end).is_empty());
    let timer = observe(&mut hub, "m5", observed(SessionState::Idle, NOW + 400_000));
    assert!(snapshots(&timer).is_empty());
    let ready = hub.local(
        &m5,
        LocalCmd::Snapshot {
            trigger: Trigger::Ready(id("m1")),
        },
    );
    assert!(snapshots(&ready).is_empty());
    assert!(matches!(answers(&ready)[0], Err(e) if e.code == "refused"));
    let last = hub.local(
        &m5,
        LocalCmd::Snapshot {
            trigger: Trigger::Final,
        },
    );
    assert_eq!(snapshots(&last).len(), 1);

    let lifted = hub.local(&m5, LocalCmd::Fence { reason: None });
    let asks = snapshots(&lifted);
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0].trigger, Trigger::Ready(id("m1")));
}
