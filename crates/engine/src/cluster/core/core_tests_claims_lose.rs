use serde_json::Value;

use super::core_tests::{answers, Hub};
use super::{Effect, LocalCmd};
use crate::cluster::claims::Claim;
use crate::cluster::id::NodeId;
use crate::cluster::members::Term;
use crate::cluster::state::Command;
use crate::cluster::testkit::id;
use crate::cluster::wire::{OriginTier, WireError};

fn pair() -> (Hub, NodeId, NodeId) {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.tick_all(6_000);
    hub.tick_all(6_000);
    (hub, m5, m1)
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

fn record(hub: &mut Hub, who: &NodeId, c: Claim) -> Vec<Effect> {
    hub.local(
        who,
        LocalCmd::Record {
            command: Command::Claim { claim: c },
            tier: OriginTier::Node,
        },
    )
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

#[test]
fn a_losing_claim_on_the_home_is_refused() {
    let (mut hub, m5, m1) = pair();
    record(&mut hub, &m1, claim("s1", &m1, 3));
    let effects = record(&mut hub, &m5, claim("s1", &m5, 2));
    let err = answers(&effects)[0].clone().expect_err("refused");
    assert_eq!(err.code, "refused");
    assert!(err.message.contains("newer claim"), "{}", err.message);
}

#[test]
fn a_losing_claim_from_a_member_is_refused_through_the_home() {
    let (mut hub, m5, m1) = pair();
    record(&mut hub, &m5, claim("s1", &m5, 3));
    let mark = hub.seen.len();
    record(&mut hub, &m1, claim("s1", &m1, 2));
    let err = answered(&hub, mark, "m1").expect_err("refused");
    assert_eq!(err.code, "refused");
}

#[test]
fn a_retry_of_the_winning_claim_still_answers_ok() {
    let (mut hub, _, m1) = pair();
    let mark = hub.seen.len();
    record(&mut hub, &m1, claim("s1", &m1, 3));
    assert_eq!(answered(&hub, mark, "m1").expect("ok")["changed"], true);
    let mark = hub.seen.len();
    record(&mut hub, &m1, claim("s1", &m1, 3));
    assert_eq!(answered(&hub, mark, "m1").expect("ok")["changed"], false);
    let mark = hub.seen.len();
    record(&mut hub, &m1, claim("s1", &m1, 2));
    assert_eq!(answered(&hub, mark, "m1").expect("ok")["changed"], false);
}

#[test]
fn a_members_claim_is_journaled_on_it_before_its_record_is_answered() {
    let (mut hub, _, m1) = pair();
    let mark = hub.seen.len();
    record(&mut hub, &m1, claim("s1", &m1, 1));
    let mine: Vec<&Effect> = hub.seen[mark..]
        .iter()
        .filter(|(n, _)| n == "m1")
        .map(|(_, e)| e)
        .collect();
    let saved = mine.iter().position(|e| {
        matches!(e, Effect::Persist { entry, .. } if matches!(entry.command, Command::Claim { .. }))
    });
    let reply = mine
        .iter()
        .position(|e| matches!(e, Effect::Answer { result: Ok(_), .. }));
    assert!(saved.is_some() && reply.is_some(), "{mine:?}");
    assert!(saved < reply, "{mine:?}");
}
