use std::collections::BTreeMap;

use super::core_tests::{answers, Hub};
use super::{Effect, LocalCmd, Takeover};
use crate::cluster::claims::Claim;
use crate::cluster::id::NodeId;
use crate::cluster::members::{MemberState, Term};
use crate::cluster::state::Change;
use crate::cluster::testkit::id;

fn trio() -> (Hub, NodeId, NodeId) {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.join("m5", "w1", "linux", false);
    hub.link(&m1, &id("w1"));
    hub.tick_all(6_000);
    hub.tick_all(6_000);
    (hub, m5, m1)
}

fn takeover_for(hub: &Hub, from: &NodeId, to: &NodeId) -> Takeover {
    let state = hub.core(from).state().expect("state");
    Takeover {
        id: state.id.clone(),
        name: state.name.clone(),
        term: Term {
            n: state.term.n + 1,
            node: to.clone(),
        },
        vault: state.vault.clone(),
        members: hub.core(from).shared_members(),
        sessions: Vec::new(),
        set: None,
        claims: Vec::new(),
        restored: None,
        staged: false,
    }
}

fn take(hub: &mut Hub, who: &NodeId, takeover: Takeover) -> Vec<Effect> {
    hub.local(
        who,
        LocalCmd::TakePrimary {
            takeover: Box::new(takeover),
        },
    )
}

fn claim(session: &str, node: &NodeId, n: u64, term_n: u64, at: &NodeId) -> Claim {
    Claim {
        session: session.to_string(),
        node: node.clone(),
        n,
        term: Term {
            n: term_n,
            node: at.clone(),
        },
        base_set: "set-1".to_string(),
        base_len: 10,
        touched: false,
    }
}

fn claims_of(hub: &Hub, who: &NodeId) -> BTreeMap<String, Claim> {
    hub.core(who).state().expect("state").claims.clone()
}

fn persisted_claims(effects: &[Effect]) -> Option<BTreeMap<String, Claim>> {
    effects.iter().find_map(|e| match e {
        Effect::Persist { changes, .. } => changes.iter().find_map(|c| match c {
            Change::Claims(claims) => Some(claims.clone()),
            _ => None,
        }),
        _ => None,
    })
}

#[test]
fn a_claim_by_a_node_that_is_not_a_member_is_not_merged() {
    let (mut hub, m5, m1) = trio();
    let stranger = id("zz");
    let mut takeover = takeover_for(&hub, &m5, &m1);
    takeover.claims = vec![
        claim("sess-ok", &m5, 3, 1, &m5),
        claim("sess-stranger", &stranger, 9, 1, &m5),
    ];
    let effects = take(&mut hub, &m1, takeover);
    assert!(answers(&effects)[0].is_ok());
    let held = claims_of(&hub, &m1);
    assert!(held.contains_key("sess-ok"), "{held:?}");
    assert!(!held.contains_key("sess-stranger"), "{held:?}");
    let stored = persisted_claims(&effects).expect("the kept claim is persisted");
    assert!(!stored.contains_key("sess-stranger"), "{stored:?}");
}

#[test]
fn a_claim_by_a_removed_member_is_not_merged() {
    let (mut hub, m5, m1) = trio();
    let w1 = id("w1");
    let mut takeover = takeover_for(&hub, &m5, &m1);
    for member in takeover.members.iter_mut().filter(|m| m.id == w1) {
        member.state = MemberState::Removed;
    }
    takeover.claims = vec![
        claim("sess-gone", &w1, 4, 1, &m5),
        claim("sess-ok", &m1, 2, 1, &m5),
    ];
    let effects = take(&mut hub, &m1, takeover);
    assert!(answers(&effects)[0].is_ok());
    let held = claims_of(&hub, &m1);
    assert!(held.contains_key("sess-ok"), "{held:?}");
    assert!(!held.contains_key("sess-gone"), "{held:?}");
}

#[test]
fn a_claim_whose_term_cannot_be_stored_is_not_merged() {
    let (mut hub, m5, m1) = trio();
    let mut takeover = takeover_for(&hub, &m5, &m1);
    takeover.claims = vec![
        claim("sess-huge", &m5, 5, u64::MAX, &m5),
        claim("sess-ok", &m5, 1, 1, &m5),
    ];
    let effects = take(&mut hub, &m1, takeover);
    assert!(answers(&effects)[0].is_ok());
    let held = claims_of(&hub, &m1);
    assert!(held.contains_key("sess-ok"), "{held:?}");
    assert!(!held.contains_key("sess-huge"), "{held:?}");
}
