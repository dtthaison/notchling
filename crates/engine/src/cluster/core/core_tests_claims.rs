use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::core_tests::{answers, Hub};
use super::{Effect, Input, LocalCmd};
use crate::cluster::claims::{Claim, SideCopy};
use crate::cluster::id::NodeId;
use crate::cluster::members::Term;
use crate::cluster::persist::{load, save};
use crate::cluster::state::{Change, Command};
use crate::cluster::testkit::id;
use crate::cluster::wire::{Msg, OriginTier, WireError};
use crate::sessions::testutil::TempDir;

fn trio() -> (Hub, NodeId, NodeId, NodeId) {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.join("m5", "w1", "linux", false);
    hub.tick_all(6_000);
    hub.tick_all(6_000);
    (hub, m5, m1, w1)
}

fn claim(session: &str, node: &NodeId, n: u64) -> Claim {
    Claim {
        session: session.to_string(),
        node: node.clone(),
        n,
        term: Term {
            n: 1,
            node: id("m5"),
        },
        base_set: "set-1".to_string(),
        base_len: 10,
        touched: false,
    }
}

fn copy(session: &str, node: &NodeId) -> SideCopy {
    SideCopy {
        session: session.to_string(),
        node: node.clone(),
        base_set: "set-1".to_string(),
        extra_bytes: 40,
        repo_changed: false,
        path: None,
        at_ms: 7,
        hidden: false,
    }
}

fn record(hub: &mut Hub, who: &NodeId, command: Command, tier: OriginTier) -> Vec<Effect> {
    hub.local(who, LocalCmd::Record { command, tier })
}

fn record_node(hub: &mut Hub, who: &NodeId, command: Command) -> Vec<Effect> {
    record(hub, who, command, OriginTier::Node)
}

fn record_app(hub: &mut Hub, who: &NodeId, command: Command) -> Vec<Effect> {
    record(hub, who, command, OriginTier::App)
}

fn claim_cmd(c: Claim) -> Command {
    Command::Claim { claim: c }
}

fn answered(hub: &Hub, mark: usize, who: &str) -> Result<Value, WireError> {
    hub.seen[mark..]
        .iter()
        .filter(|(n, _)| n == who)
        .find_map(|(_, e)| match e {
            Effect::Answer { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("an answer")
}

fn claims_of(hub: &Hub, who: &NodeId) -> BTreeMap<String, Claim> {
    hub.core(who).state().expect("state").claims.clone()
}

fn copies_of(hub: &Hub, who: &NodeId) -> Vec<SideCopy> {
    hub.core(who).state().expect("state").side_copies.clone()
}

fn journaled(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Persist { .. }))
        .count()
}

fn record_entries(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| match e {
            Effect::Persist { entry, .. } => matches!(
                entry.command,
                Command::Claim { .. }
                    | Command::Touched { .. }
                    | Command::SideCopy { .. }
                    | Command::HideSideCopy { .. }
            ),
            _ => false,
        })
        .count()
}

fn members_msg(hub: &Hub, from: &NodeId, claims: Vec<Claim>, copies: Vec<SideCopy>) -> Msg {
    let state = hub.core(from).state().expect("state");
    Msg::Members {
        term: state.term.clone(),
        home: from.clone(),
        vault: Some(state.vault.clone()),
        members: state.members.values().cloned().collect(),
        failover: Some(state.failover),
        armed: false,
        claims,
        side_copies: copies,
    }
}

fn deliver(hub: &mut Hub, to: &NodeId, from: &NodeId, msg: Msg) -> Vec<Effect> {
    hub.nodes.get_mut(to).expect("node").step(Input::Msg {
        from: from.clone(),
        msg,
    })
}

#[test]
fn members_push_carries_claims_and_side_copies() {
    let (mut hub, m5, m1, w1) = trio();
    record_node(&mut hub, &m5, claim_cmd(claim("s1", &m5, 2)));
    record_node(
        &mut hub,
        &m5,
        Command::SideCopy {
            copy: copy("s2", &m5),
        },
    );
    record_app(
        &mut hub,
        &m5,
        Command::HideSideCopy {
            session: "s2".to_string(),
            node: m5.clone(),
        },
    );
    let pushes: Vec<(&Vec<Claim>, &Vec<SideCopy>)> = hub
        .effects_of("m5")
        .into_iter()
        .filter_map(|e| match e {
            Effect::Send {
                to,
                msg:
                    Msg::Members {
                        claims,
                        side_copies,
                        ..
                    },
            } if *to == m1 => Some((claims, side_copies)),
            _ => None,
        })
        .collect();
    let (claims, copies) = pushes.last().expect("a push");
    assert_eq!(claims.len(), 1);
    assert_eq!(copies.len(), 1);
    assert!(copies[0].hidden);
    for who in [&m1, &w1] {
        assert_eq!(claims_of(&hub, who)["s1"].n, 2);
        let held = copies_of(&hub, who);
        assert_eq!(held.len(), 1);
        assert!(held[0].hidden);
    }
}

#[test]
fn follower_keeps_the_winning_claim() {
    let (mut hub, m5, m1, _) = trio();
    record_node(&mut hub, &m5, claim_cmd(claim("s1", &m5, 5)));
    assert_eq!(claims_of(&hub, &m1)["s1"].n, 5);
    let lower = members_msg(&hub, &m5, vec![claim("s1", &m5, 2)], Vec::new());
    let effects = deliver(&mut hub, &m1, &m5, lower);
    assert_eq!(claims_of(&hub, &m1)["s1"].n, 5);
    assert_eq!(record_entries(&effects), 0);
    let higher = members_msg(&hub, &m5, vec![claim("s1", &m5, 9)], Vec::new());
    deliver(&mut hub, &m1, &m5, higher);
    assert_eq!(claims_of(&hub, &m1)["s1"].n, 9);
}

#[test]
fn older_members_push_changes_no_claim() {
    let (mut hub, m5, m1, _) = trio();
    record_node(&mut hub, &m5, claim_cmd(claim("s1", &m5, 5)));
    record_node(
        &mut hub,
        &m5,
        Command::SideCopy {
            copy: copy("s1", &m5),
        },
    );
    let mut line =
        serde_json::to_value(members_msg(&hub, &m5, Vec::new(), Vec::new())).expect("serialized");
    let fields = line.as_object_mut().expect("an object");
    assert!(fields.remove("claims").is_some());
    assert!(fields.remove("side_copies").is_some());
    let old: Msg = serde_json::from_value(line).expect("an older line still reads");
    let effects = deliver(&mut hub, &m1, &m5, old);
    assert_eq!(claims_of(&hub, &m1)["s1"].n, 5);
    assert_eq!(copies_of(&hub, &m1).len(), 1);
    assert_eq!(record_entries(&effects), 0);
}

#[test]
fn inadmissible_pushed_claim_is_dropped() {
    let (mut hub, m5, m1, w1) = trio();
    let mut huge = claim("s-huge", &m5, 3);
    huge.term.n = u64::MAX;
    let pushed = vec![
        claim("s-ok", &m5, 1),
        claim("s-stranger", &id("zz"), 9),
        huge,
    ];
    let msg = members_msg(&hub, &m5, pushed, Vec::new());
    deliver(&mut hub, &m1, &m5, msg);
    let held = claims_of(&hub, &m1);
    assert!(held.contains_key("s-ok"), "{held:?}");
    assert!(!held.contains_key("s-stranger"), "{held:?}");
    assert!(!held.contains_key("s-huge"), "{held:?}");
    assert!(claims_of(&hub, &w1).is_empty());
}

#[test]
fn member_records_its_own_claim_through_the_home() {
    let (mut hub, m5, m1, w1) = trio();
    let mark = hub.seen.len();
    let effects = record_node(&mut hub, &m1, claim_cmd(claim("s1", &m1, 1)));
    let relayed = effects.iter().any(|e| {
        matches!(e, Effect::Send { to, msg: Msg::Call { op, .. } } if *to == m5 && op == "cluster.record")
    });
    assert!(relayed, "{effects:?}");
    assert_eq!(journaled(&effects), 0);
    let home_journaled = hub.seen[mark..].iter().any(|(n, e)| {
        n == "m5"
            && matches!(e, Effect::Persist { entry, .. } if matches!(entry.command, Command::Claim { .. }))
    });
    assert!(home_journaled);
    assert_eq!(answered(&hub, mark, "m1").expect("ok")["changed"], true);
    for who in [&m5, &m1, &w1] {
        assert_eq!(claims_of(&hub, who)["s1"].node, m1);
    }
}

#[test]
fn home_records_its_own_claim_and_pushes() {
    let (mut hub, m5, m1, _) = trio();
    let effects = record_node(&mut hub, &m5, claim_cmd(claim("s1", &m5, 1)));
    assert_eq!(answers(&effects)[0].as_ref().expect("ok")["changed"], true);
    assert_eq!(journaled(&effects), 1);
    let pushed = effects.iter().any(|e| {
        matches!(e, Effect::Send { to, msg: Msg::Members { claims, .. } } if *to == m1 && claims.len() == 1)
    });
    assert!(pushed, "{effects:?}");
    assert_eq!(claims_of(&hub, &m5)["s1"].node, m5);
}

#[test]
fn claim_for_another_node_is_refused() {
    let (mut hub, m5, m1, _) = trio();
    let mark = hub.seen.len();
    record_node(&mut hub, &m1, claim_cmd(claim("s1", &m5, 1)));
    let err = answered(&hub, mark, "m1").expect_err("refused");
    assert_eq!(err.code, "refused");
    assert!(claims_of(&hub, &m5).is_empty());
    assert!(claims_of(&hub, &m1).is_empty());
    let home_journaled = hub.seen[mark..]
        .iter()
        .any(|(n, e)| n == "m5" && matches!(e, Effect::Persist { .. }));
    assert!(!home_journaled);
    let mut huge = claim("s2", &m1, 1);
    huge.term.n = u64::MAX;
    let mark = hub.seen.len();
    record_node(&mut hub, &m1, claim_cmd(huge));
    assert!(answered(&hub, mark, "m1").is_err());
    assert!(claims_of(&hub, &m5).is_empty());
}

#[test]
fn touched_only_by_the_owner() {
    let (mut hub, m5, m1, w1) = trio();
    record_node(&mut hub, &m1, claim_cmd(claim("s1", &m1, 1)));
    let touch = || Command::Touched {
        session: "s1".to_string(),
    };
    let mark = hub.seen.len();
    record_node(&mut hub, &w1, touch());
    assert!(answered(&hub, mark, "w1").is_err());
    assert!(!claims_of(&hub, &m5)["s1"].touched);
    let mark = hub.seen.len();
    record_node(&mut hub, &m5, touch());
    assert!(answered(&hub, mark, "m5").is_err());
    assert!(!claims_of(&hub, &m5)["s1"].touched);
    let mark = hub.seen.len();
    record_node(
        &mut hub,
        &m1,
        Command::Touched {
            session: "nobody".to_string(),
        },
    );
    assert!(answered(&hub, mark, "m1").is_err());
    let mark = hub.seen.len();
    record_node(&mut hub, &m1, touch());
    assert_eq!(answered(&hub, mark, "m1").expect("ok")["changed"], true);
    for who in [&m5, &m1, &w1] {
        assert!(claims_of(&hub, who)["s1"].touched);
    }
}

#[test]
fn side_copy_recorded_then_hidden_never_removed() {
    let (mut hub, m5, m1, w1) = trio();
    let mark = hub.seen.len();
    record_node(
        &mut hub,
        &m1,
        Command::SideCopy {
            copy: copy("s1", &m1),
        },
    );
    assert!(answered(&hub, mark, "m1").is_ok());
    for who in [&m5, &m1, &w1] {
        let held = copies_of(&hub, who);
        assert_eq!(held.len(), 1);
        assert!(!held[0].hidden);
    }
    let mark = hub.seen.len();
    record_app(
        &mut hub,
        &m1,
        Command::HideSideCopy {
            session: "s1".to_string(),
            node: m1.clone(),
        },
    );
    assert_eq!(answered(&hub, mark, "m1").expect("ok")["changed"], true);
    for who in [&m5, &m1, &w1] {
        let held = copies_of(&hub, who);
        assert_eq!(held.len(), 1);
        assert!(held[0].hidden);
    }
    record_node(
        &mut hub,
        &m1,
        Command::SideCopy {
            copy: copy("s1", &m1),
        },
    );
    for who in [&m5, &m1, &w1] {
        let held = copies_of(&hub, who);
        assert_eq!(held.len(), 1);
        assert!(held[0].hidden);
    }
}

#[test]
fn hide_needs_app_origin() {
    let (mut hub, m5, m1, _) = trio();
    record_node(
        &mut hub,
        &m1,
        Command::SideCopy {
            copy: copy("s1", &m1),
        },
    );
    let hide = Command::HideSideCopy {
        session: "s1".to_string(),
        node: m1.clone(),
    };
    for who in [&m1, &m5] {
        let mark = hub.seen.len();
        record_node(&mut hub, who, hide.clone());
        let name = if *who == m1 { "m1" } else { "m5" };
        assert_eq!(
            answered(&hub, mark, name).expect_err("refused").code,
            "refused"
        );
        assert!(!copies_of(&hub, &m5)[0].hidden);
    }
    let mark = hub.seen.len();
    record_app(
        &mut hub,
        &m1,
        Command::HideSideCopy {
            session: "nothing".to_string(),
            node: m1.clone(),
        },
    );
    assert!(answered(&hub, mark, "m1").is_err());
    assert!(!copies_of(&hub, &m5)[0].hidden);
    record_app(&mut hub, &m1, hide);
    assert!(copies_of(&hub, &m5)[0].hidden);
}

#[test]
fn a_side_copy_cannot_be_recorded_hidden_or_for_another_node() {
    let (mut hub, m5, m1, _) = trio();
    let mut hidden = copy("s1", &m1);
    hidden.hidden = true;
    for bad in [hidden, copy("s2", &m5)] {
        let mark = hub.seen.len();
        record_node(&mut hub, &m1, Command::SideCopy { copy: bad });
        assert!(answered(&hub, mark, "m1").is_err());
    }
    assert!(copies_of(&hub, &m5).is_empty());
}

#[test]
fn stranger_or_removed_member_cannot_record() {
    let (mut hub, m5, _, w1) = trio();
    let mut out = Vec::new();
    let stranger = id("zz");
    let result = hub.core_mut(&m5).record(
        &stranger,
        OriginTier::Node,
        claim_cmd(claim("s1", &stranger, 1)),
        &mut out,
    );
    assert_eq!(result.expect_err("refused").code, "forbidden");
    hub.local(&m5, LocalCmd::Remove { node: w1.clone() });
    let result = hub.core_mut(&m5).record(
        &w1,
        OriginTier::Node,
        claim_cmd(claim("s1", &w1, 1)),
        &mut out,
    );
    assert_eq!(result.expect_err("refused").code, "forbidden");
    assert!(out.is_empty());
    assert!(claims_of(&hub, &m5).is_empty());
}

#[test]
fn a_call_from_a_worker_with_the_wrong_origin_is_refused_by_policy() {
    let (mut hub, m5, _, w1) = trio();
    let args = json!({"command": claim_cmd(claim("s1", &w1, 1))});
    let call = Msg::Call {
        id: 99,
        op: "cluster.record".to_string(),
        args: args.into(),
        origin: crate::cluster::wire::Origin {
            node: w1.clone(),
            tier: OriginTier::App,
        },
        term: hub.core(&m5).state().expect("state").term.clone(),
    };
    deliver(&mut hub, &m5, &w1, call);
    assert!(claims_of(&hub, &m5).is_empty());
}

#[test]
fn record_of_another_command_is_bad_args() {
    let (mut hub, m5, m1, _) = trio();
    for who in [&m5, &m1] {
        let name = if *who == m5 { "m5" } else { "m1" };
        let mark = hub.seen.len();
        let effects = record_app(
            &mut hub,
            who,
            Command::SetMain {
                session: "s1".to_string(),
                main: true,
            },
        );
        assert_eq!(
            answered(&hub, mark, name).expect_err("bad").code,
            "bad_args"
        );
        assert_eq!(journaled(&effects), 0);
        assert!(!effects.iter().any(|e| matches!(e, Effect::Send { .. })));
    }
    assert!(hub.core(&m5).state().expect("state").main.is_empty());
}

#[test]
fn a_relayed_record_of_another_command_is_bad_args_at_the_home() {
    let (mut hub, m5, m1, _) = trio();
    let args = json!({"command": Command::SetMain { session: "s1".to_string(), main: true }});
    let call = Msg::Call {
        id: 77,
        op: "cluster.record".to_string(),
        args: args.into(),
        origin: crate::cluster::wire::Origin {
            node: m1.clone(),
            tier: OriginTier::Node,
        },
        term: hub.core(&m5).state().expect("state").term.clone(),
    };
    let effects = deliver(&mut hub, &m5, &m1, call);
    let bad = effects.iter().any(|e| {
        matches!(e, Effect::Send { msg: Msg::Reply { err: Some(err), .. }, .. } if err.code == "bad_args")
    });
    assert!(bad, "{effects:?}");
    assert!(hub.core(&m5).state().expect("state").main.is_empty());
}

#[test]
fn record_that_changes_nothing_writes_no_entry() {
    let (mut hub, m5, m1, _) = trio();
    record_node(&mut hub, &m5, claim_cmd(claim("s1", &m5, 3)));
    let effects = record_node(&mut hub, &m5, claim_cmd(claim("s1", &m5, 3)));
    assert_eq!(answers(&effects)[0].as_ref().expect("ok")["changed"], false);
    assert_eq!(journaled(&effects), 0);
    assert!(!effects.iter().any(|e| matches!(e, Effect::Send { .. })));
    let mark = hub.seen.len();
    let effects = record_node(&mut hub, &m1, claim_cmd(claim("s1", &m1, 1)));
    assert_eq!(journaled(&effects), 0);
    assert!(answered(&hub, mark, "m1").is_err());
    let home_wrote = hub.seen[mark..]
        .iter()
        .any(|(n, e)| n == "m5" && matches!(e, Effect::Persist { .. }));
    assert!(!home_wrote);
    assert_eq!(claims_of(&hub, &m5)["s1"].node, m5);
}

#[test]
fn status_lists_claims_and_side_copies() {
    let (mut hub, m5, m1, _) = trio();
    assert_eq!(hub.core(&m5).status()["claims"], json!([]));
    assert_eq!(hub.core(&m5).status()["sideCopies"], json!([]));
    record_node(&mut hub, &m5, claim_cmd(claim("s1", &m5, 2)));
    record_node(
        &mut hub,
        &m1,
        Command::SideCopy {
            copy: copy("s2", &m1),
        },
    );
    record_app(
        &mut hub,
        &m1,
        Command::HideSideCopy {
            session: "s2".to_string(),
            node: m1.clone(),
        },
    );
    for who in [&m5, &m1] {
        let status = hub.core(who).status();
        let claims = status["claims"].as_array().expect("claims");
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0]["session"], "s1");
        assert_eq!(claims[0]["n"], 2);
        assert_eq!(claims[0]["touched"], false);
        let copies = status["sideCopies"].as_array().expect("copies");
        assert_eq!(copies.len(), 1);
        assert_eq!(copies[0]["session"], "s2");
        assert_eq!(copies[0]["hidden"], true);
    }
}

#[test]
fn a_home_record_persists_and_survives_a_round_trip() {
    let (mut hub, m5, m1, _) = trio();
    let mark = hub.seen.len();
    record_node(&mut hub, &m5, claim_cmd(claim("s1", &m5, 2)));
    record_node(
        &mut hub,
        &m1,
        Command::SideCopy {
            copy: copy("s2", &m1),
        },
    );
    record_app(
        &mut hub,
        &m5,
        Command::HideSideCopy {
            session: "s2".to_string(),
            node: m1.clone(),
        },
    );
    let persists: Vec<(&crate::cluster::state::JournalEntry, &Vec<Change>)> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "m5")
        .filter_map(|(_, e)| match e {
            Effect::Persist { entry, changes } => Some((entry, changes)),
            _ => None,
        })
        .collect();
    let since: Vec<_> = hub.seen[mark..]
        .iter()
        .filter(|(n, _)| n == "m5")
        .filter_map(|(_, e)| match e {
            Effect::Persist { changes, .. } => Some(changes),
            _ => None,
        })
        .collect();
    assert!(since
        .iter()
        .any(|c| c.iter().any(|c| matches!(c, Change::Claims(_)))));
    assert!(since
        .iter()
        .any(|c| c.iter().any(|c| matches!(c, Change::SideCopies(_)))));
    let tmp = TempDir::new("cluster-claims");
    let dir = tmp.path("cluster");
    for (entry, changes) in persists {
        save(&dir, entry, changes).expect("save");
    }
    let loaded = load(&dir).expect("load").expect("state");
    let state = hub.core(&m5).state().expect("state");
    assert_eq!(loaded.claims, state.claims);
    assert_eq!(loaded.side_copies, state.side_copies);
    assert!(loaded.side_copies[0].hidden);
}
