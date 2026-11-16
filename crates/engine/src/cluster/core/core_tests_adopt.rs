use serde_json::json;

use super::core_tests::Hub;
use super::{Effect, Input, LocalCmd, NodeCore, MAX_TERM_STEP};
use crate::cluster::id::NodeId;
use crate::cluster::members::{Term, MAX_TERM_N};
use crate::cluster::testkit::{id, versions};
use crate::cluster::wire::{Msg, Origin, OriginTier, WireError};

pub(super) struct Cast {
    pub(super) hub: Hub,
    pub(super) h: NodeId,
    pub(super) x: NodeId,
    pub(super) n: NodeId,
    pub(super) w: NodeId,
}

pub(super) fn stranded() -> Cast {
    let mut hub = Hub::new();
    let h = hub.add("m5", "macos", true);
    let x = hub.add("x1", "macos", true);
    let n = hub.add("n1", "macos", true);
    let w = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join("m5", "x1", "macos", false);
    hub.join("m5", "n1", "macos", false);
    hub.join("m5", "w1", "linux", false);
    hub.linked
        .retain(|(a, b)| !((a == &h && b == &n) || (a == &n && b == &h)));
    hub.local(
        &h,
        LocalCmd::SetMember {
            node: x.clone(),
            can_be_master: Some(true),
            client: None,
            reference: None,
            accepts_delegation: None,
            headless: None,
            desired: None,
        },
    );
    promote(&mut hub, &x);
    hub.linked.push((h.clone(), n.clone()));
    Cast { hub, h, x, n, w }
}

fn promote(hub: &mut Hub, who: &NodeId) {
    let effects = hub.nodes.get_mut(who).expect("node").bump_term();
    for effect in effects {
        if let Effect::Send { to, msg } = effect {
            if hub.is_linked(who, &to) {
                hub.run(
                    &to,
                    Input::Msg {
                        from: who.clone(),
                        msg,
                    },
                );
            }
        }
    }
}

pub(super) fn newer() -> Term {
    Term {
        n: 2,
        node: id("x1"),
    }
}

fn inject(hub: &mut Hub, to: &NodeId, from: &NodeId, msg: Msg) -> Vec<Effect> {
    hub.run(
        to,
        Input::Msg {
            from: from.clone(),
            msg,
        },
    )
}

fn hello_ok(from: &NodeId, term: Term) -> Msg {
    Msg::HelloOk {
        node: from.clone(),
        cluster: "c1".to_string(),
        home: term.node.clone(),
        term,
    }
}

fn hello(from: &NodeId, term: Term) -> Msg {
    Msg::Hello {
        node: from.clone(),
        cluster: Some("c1".to_string()),
        versions: versions("macos"),
        boot_id: "boot".to_string(),
        term: Some(term),
    }
}

fn stale_refusal(term: Term) -> Msg {
    Msg::Refused {
        reason: "stale_term".to_string(),
        home: Some(term.node.clone()),
        term: Some(term),
    }
}

fn call_stamped(from: &NodeId, term: Term) -> Msg {
    Msg::Call {
        id: 77,
        op: "node.load".to_string(),
        args: json!({}).into(),
        origin: Origin {
            node: from.clone(),
            tier: OriginTier::Node,
        },
        term,
    }
}

fn members_push(hub: &Hub, from: &NodeId, term: Term) -> Msg {
    Msg::Members {
        home: term.node.clone(),
        term,
        vault: None,
        members: hub
            .core(from)
            .state()
            .expect("state")
            .members
            .values()
            .cloned()
            .collect(),
        failover: None,
        armed: false,
        claims: Vec::new(),
        side_copies: Vec::new(),
    }
}

fn reply_stale(hub: &mut Hub, n: &NodeId, from: &NodeId, term: Term) {
    let effects = hub.local(
        n,
        LocalCmd::Forward {
            node: from.clone(),
            op: "node.load".to_string(),
            args: json!({}).into(),
            origin: Origin {
                node: n.clone(),
                tier: OriginTier::Node,
            },
        },
    );
    let call = effects
        .iter()
        .find_map(|e| match e {
            Effect::Send {
                msg: Msg::Call { id, .. },
                ..
            } => Some(*id),
            _ => None,
        })
        .expect("a call went out");
    let body = json!({"term": term, "home": term.node}).to_string();
    inject(
        hub,
        n,
        from,
        Msg::Reply {
            id: call,
            ok: None,
            err: Some(WireError::new("stale_term", body)),
        },
    );
}

pub(super) fn relink(hub: &mut Hub, a: &NodeId, b: &NodeId) {
    if !hub.is_linked(a, b) {
        hub.linked.push((a.clone(), b.clone()));
    }
    hub.run(a, Input::Linked { peer: b.clone() });
    hub.run(b, Input::Linked { peer: a.clone() });
}

pub(super) fn unmoved(c: &Cast) -> bool {
    let state = c.hub.core(&c.n).state().expect("state");
    state.home == c.h && state.term.n == 1
}

pub(super) fn follows_x_with_its_members(c: &Cast) -> bool {
    let state = c.hub.core(&c.n).state().expect("state");
    state.home == c.x
        && state.term == newer()
        && state.members.get(&c.x).is_some_and(|m| m.can_be_master)
}

#[test]
fn the_stranded_node_starts_out_not_seeing_the_grant() {
    let c = stranded();
    let state = c.hub.core(&c.n).state().expect("state");
    assert!(!state.members[&c.x].can_be_master);
    assert!(unmoved(&c));
    let home = c.hub.core(&c.h).state().expect("state");
    assert_eq!(home.home, c.x);
    assert_eq!(home.term, newer());
}

#[test]
fn the_old_home_vouching_through_hello_ok_moves_a_stranded_node() {
    let mut c = stranded();
    inject(&mut c.hub, &c.n, &c.h, hello_ok(&c.h, newer()));
    let state = c.hub.core(&c.n).state().expect("state");
    assert_eq!(state.home, c.x);
    assert_eq!(state.term, newer());
    relink(&mut c.hub, &c.n, &c.x);
    assert!(follows_x_with_its_members(&c));
    assert!(c.hub.core(&c.n).online().contains(&c.x));
}

#[test]
fn the_old_home_vouching_through_a_stale_term_refusal_moves_a_stranded_node() {
    let mut c = stranded();
    let effects = inject(&mut c.hub, &c.n, &c.h, stale_refusal(newer()));
    assert!(effects.iter().any(|e| matches!(e, Effect::Persist { .. })));
    assert_eq!(c.hub.core(&c.n).state().expect("state").home, c.x);
    relink(&mut c.hub, &c.n, &c.x);
    assert!(follows_x_with_its_members(&c));
}

#[test]
fn the_old_home_vouching_through_a_stale_term_reply_moves_a_stranded_node() {
    let mut c = stranded();
    reply_stale(&mut c.hub, &c.n, &c.h, newer());
    assert_eq!(c.hub.core(&c.n).state().expect("state").home, c.x);
    relink(&mut c.hub, &c.n, &c.x);
    assert!(follows_x_with_its_members(&c));
}

#[test]
fn the_real_exchange_with_the_old_home_moves_a_stranded_node() {
    let mut c = stranded();
    relink(&mut c.hub, &c.n, &c.h);
    assert_eq!(c.hub.core(&c.n).state().expect("state").home, c.x);
    relink(&mut c.hub, &c.n, &c.x);
    assert!(follows_x_with_its_members(&c));
}

#[test]
fn the_old_home_vouching_through_a_hello_or_a_call_moves_a_stranded_node() {
    let mut c = stranded();
    let (n, h) = (c.n.clone(), c.h.clone());
    let effects = inject(&mut c.hub, &n, &h, hello(&h, newer()));
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Send {
            msg: Msg::HelloOk { .. },
            ..
        }
    )));
    assert_eq!(c.hub.core(&n).state().expect("state").home, c.x);

    let mut c = stranded();
    let (n, h) = (c.n.clone(), c.h.clone());
    inject(&mut c.hub, &n, &h, call_stamped(&h, newer()));
    assert_eq!(c.hub.core(&n).state().expect("state").home, c.x);
}

fn every_forged_route(c: &mut Cast, sender: &NodeId, named: &Term) {
    let n = c.n.clone();
    inject(&mut c.hub, &n, sender, hello(sender, named.clone()));
    assert!(unmoved(c), "hello from {sender}");
    inject(&mut c.hub, &n, sender, hello_ok(sender, named.clone()));
    assert!(unmoved(c), "hello_ok from {sender}");
    inject(&mut c.hub, &n, sender, stale_refusal(named.clone()));
    assert!(unmoved(c), "refused from {sender}");
    let own = c.hub.core(&n).state().expect("state").term.clone();
    inject(&mut c.hub, &n, sender, hello_ok(sender, own));
    inject(&mut c.hub, &n, sender, call_stamped(sender, named.clone()));
    assert!(unmoved(c), "call from {sender}");
    let push = members_push(&c.hub, &c.x, named.clone());
    inject(&mut c.hub, &n, sender, push);
    assert!(unmoved(c), "members from {sender}");
    reply_stale(&mut c.hub, &n, sender, named.clone());
    assert!(unmoved(c), "reply from {sender}");
}

#[test]
fn the_new_primary_alone_cannot_move_a_node_that_does_not_know_it_may_promote() {
    let mut c = stranded();
    let (x, named) = (c.x.clone(), newer());
    every_forged_route(&mut c, &x, &named);
    let state = c.hub.core(&c.n).state().expect("state");
    assert!(!state.members[&c.x].can_be_master);
}

#[test]
fn a_worker_cannot_move_a_node_by_naming_itself_or_another_member() {
    let mut c = stranded();
    let w = c.w.clone();
    let for_itself = Term {
        n: 2,
        node: w.clone(),
    };
    every_forged_route(&mut c, &w, &for_itself);
    let for_x = newer();
    every_forged_route(&mut c, &w, &for_x);
}

#[test]
fn a_node_that_is_not_a_member_cannot_move_anyone() {
    let mut c = stranded();
    let (n, nobody) = (c.n.clone(), id("nobody"));
    for msg in [
        hello(&nobody, newer()),
        hello_ok(&nobody, newer()),
        stale_refusal(newer()),
        call_stamped(&nobody, newer()),
    ] {
        let effects = c.hub.nodes.get_mut(&n).expect("node").step(Input::Msg {
            from: nobody.clone(),
            msg,
        });
        assert!(!effects.iter().any(|e| matches!(e, Effect::Persist { .. })));
        assert!(unmoved(&c));
    }
}

#[test]
fn a_vouch_for_a_node_that_is_not_an_active_member_is_refused() {
    let mut c = stranded();
    let (n, h) = (c.n.clone(), c.h.clone());
    let stranger = Term {
        n: 2,
        node: id("nobody"),
    };
    inject(&mut c.hub, &n, &h, hello_ok(&h, stranger.clone()));
    assert!(unmoved(&c));
    inject(&mut c.hub, &n, &h, stale_refusal(stranger));
    assert!(unmoved(&c));
}

pub(super) fn pair_of_promotables() -> (Hub, NodeId, NodeId) {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let w1 = hub.add("w1", "macos", true);
    hub.create("m5");
    hub.join("m5", "w1", "macos", true);
    (hub, m5, w1)
}

#[test]
fn a_jump_past_the_step_is_refused_and_the_step_itself_is_adopted() {
    for (n, adopted) in [
        (1 + MAX_TERM_STEP + 1, false),
        (1 + MAX_TERM_STEP, true),
        (u64::MAX, false),
        (MAX_TERM_N + 1, false),
    ] {
        let (mut hub, m5, w1) = pair_of_promotables();
        let term = Term {
            n,
            node: w1.clone(),
        };
        let effects = inject(&mut hub, &m5, &w1, hello_ok(&w1, term.clone()));
        let state = hub.core(&m5).state().expect("state");
        assert_eq!(state.term == term, adopted, "{n}");
        assert_eq!(
            effects.iter().any(|e| matches!(e, Effect::Persist { .. })),
            adopted,
            "{n}"
        );
    }
}

fn near_the_top() -> (NodeCore, NodeId, NodeId) {
    let (hub, m5, w1) = pair_of_promotables();
    let mut state = hub.core(&m5).state().cloned().expect("state");
    state.term.n = MAX_TERM_N - 3;
    let core = NodeCore::new(m5.clone(), versions("macos"), Some(state), true);
    (core, m5, w1)
}

#[test]
fn a_term_the_database_cannot_store_is_refused_before_it_reaches_the_journal() {
    for n in [MAX_TERM_N + 1, MAX_TERM_N + 2, u64::MAX] {
        let (mut core, m5, w1) = near_the_top();
        let own = core.state().expect("state").term.clone();
        core.step(Input::Msg {
            from: w1.clone(),
            msg: hello_ok(&w1, own),
        });
        let term = Term {
            n,
            node: w1.clone(),
        };
        for msg in [
            hello_ok(&w1, term.clone()),
            stale_refusal(term.clone()),
            hello(&w1, term.clone()),
            members_push_of(&core, &w1, term.clone()),
        ] {
            let effects = core.step(Input::Msg {
                from: w1.clone(),
                msg,
            });
            assert!(
                !effects.iter().any(|e| matches!(e, Effect::Persist { .. })),
                "{n}"
            );
            let state = core.state().expect("state");
            assert_eq!(state.term.n, MAX_TERM_N - 3, "{n}");
            assert_eq!(state.home, m5, "{n}");
        }
    }
}

fn members_push_of(core: &NodeCore, from: &NodeId, term: Term) -> Msg {
    Msg::Members {
        home: from.clone(),
        term,
        vault: None,
        members: core
            .state()
            .expect("state")
            .members
            .values()
            .cloned()
            .collect(),
        failover: None,
        armed: false,
        claims: Vec::new(),
        side_copies: Vec::new(),
    }
}

#[test]
fn the_largest_storable_term_is_adopted_from_a_promotable() {
    let (mut core, _, w1) = near_the_top();
    let term = Term {
        n: MAX_TERM_N,
        node: w1.clone(),
    };
    let effects = core.step(Input::Msg {
        from: w1.clone(),
        msg: hello_ok(&w1, term.clone()),
    });
    assert!(effects.iter().any(|e| matches!(e, Effect::Persist { .. })));
    assert_eq!(core.state().expect("state").term, term);
}

#[test]
fn a_promotes_term_is_adopted_from_each_way_it_can_arrive() {
    let routes: [fn(&NodeId, Term) -> Msg; 3] = [hello_ok, hello, |_, t| stale_refusal(t)];
    for route in routes {
        let (mut hub, m5, w1) = pair_of_promotables();
        let term = Term {
            n: 2,
            node: w1.clone(),
        };
        inject(&mut hub, &m5, &w1, route(&w1, term.clone()));
        let state = hub.core(&m5).state().expect("state");
        assert_eq!(state.home, w1);
        assert_eq!(state.term, term);
    }
    let (mut hub, m5, w1) = pair_of_promotables();
    let term = Term {
        n: 2,
        node: w1.clone(),
    };
    inject(&mut hub, &m5, &w1, call_stamped(&w1, term.clone()));
    assert_eq!(hub.core(&m5).state().expect("state").term, term);
    let (mut hub, m5, w1) = pair_of_promotables();
    reply_stale(&mut hub, &m5, &w1, term.clone());
    assert_eq!(hub.core(&m5).state().expect("state").term, term);
}

fn three_promotables() -> (Hub, NodeId, NodeId, NodeId) {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let w1 = hub.add("w1", "macos", true);
    let p2 = hub.add("p2", "macos", true);
    hub.create("m5");
    hub.join("m5", "w1", "macos", true);
    hub.join("m5", "p2", "macos", true);
    relink(&mut hub, &w1, &p2);
    (hub, m5, w1, p2)
}

fn still_at_the_first_term(hub: &Hub, who: &NodeId, home: &NodeId) -> bool {
    let state = hub.core(who).state().expect("state");
    state.home == *home && state.term.n == 1
}

#[test]
fn a_promotable_cannot_make_the_receiver_primary_by_naming_it() {
    let routes: [fn(&NodeId, Term) -> Msg; 4] =
        [hello_ok, hello, |_, t| stale_refusal(t), call_stamped];
    for (receiver, sender) in [("p2", "w1"), ("m5", "w1"), ("p2", "m5"), ("w1", "p2")] {
        for route in routes {
            let (mut hub, m5, _, _) = three_promotables();
            let (receiver, sender) = (id(receiver), id(sender));
            let named = Term {
                n: 2,
                node: receiver.clone(),
            };
            let effects = inject(&mut hub, &receiver, &sender, route(&sender, named));
            assert!(
                still_at_the_first_term(&hub, &receiver, &m5),
                "{receiver} from {sender}: {effects:?}"
            );
            assert!(!effects
                .iter()
                .any(|e| matches!(e, Effect::Persist { .. } | Effect::SteppedDown { .. })));
        }
        let (mut hub, m5, _, _) = three_promotables();
        let (receiver, sender) = (id(receiver), id(sender));
        let named = Term {
            n: 2,
            node: receiver.clone(),
        };
        reply_stale(&mut hub, &receiver, &sender, named);
        assert!(
            still_at_the_first_term(&hub, &receiver, &m5),
            "{receiver} from {sender}, reply"
        );
    }
}

#[test]
fn naming_the_receiver_is_answered_bad_term_to_a_hello_and_a_call() {
    let (mut hub, _, w1, p2) = three_promotables();
    let named = Term {
        n: 2,
        node: p2.clone(),
    };
    let effects = inject(&mut hub, &p2, &w1, hello(&w1, named.clone()));
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Send { msg: Msg::Refused { reason, .. }, .. } if reason == "bad_term"
    )));
    let (mut hub, _, w1, p2) = three_promotables();
    let effects = inject(&mut hub, &p2, &w1, call_stamped(&w1, named));
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Send { msg: Msg::Reply { err: Some(err), .. }, .. } if err.message == "bad_term"
    )));
}

#[test]
fn the_same_promotable_can_still_announce_its_own_term_to_the_receiver() {
    let (mut hub, m5, w1, p2) = three_promotables();
    let named = Term {
        n: 2,
        node: p2.clone(),
    };
    let effects = inject(&mut hub, &p2, &w1, stale_refusal(named));
    assert!(!effects
        .iter()
        .any(|e| matches!(e, Effect::Persist { .. } | Effect::SteppedDown { .. })));
    assert!(still_at_the_first_term(&hub, &p2, &m5));
    let own = Term {
        n: 2,
        node: w1.clone(),
    };
    inject(&mut hub, &p2, &w1, stale_refusal(own.clone()));
    assert_eq!(hub.core(&p2).state().expect("state").term, own);
}

#[test]
fn a_node_still_announces_its_own_term_through_its_own_promote() {
    let (mut hub, m5, w1, p2) = three_promotables();
    promote(&mut hub, &w1);
    for node in [&w1, &m5, &p2] {
        let state = hub.core(node).state().expect("state");
        assert_eq!(state.home, w1, "{node}");
        assert_eq!(state.term.node, w1, "{node}");
    }
}
