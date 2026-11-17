use serde_json::json;

use super::core_tests::{answers, pair, Hub};
use super::{Effect, Input, LocalCmd};
use crate::cluster::id::NodeId;
use crate::cluster::invite::Invite;
use crate::cluster::join::{JoinToken, MAX_TOKENS_PER_MINTER, TOKEN_TTL_MS};
use crate::cluster::members::MemberState;
use crate::cluster::testkit::{fp, id, member};
use crate::cluster::wire::{Msg, Origin, OriginTier};

fn invite_for(node: &NodeId, name: &str, can_be_master: bool, acting: bool) -> Invite {
    Invite {
        node: node.clone(),
        name: name.to_string(),
        can_be_master,
        acting,
    }
}

fn make(hub: &mut Hub, at: &NodeId, secret: &str, minter: &NodeId, invite: Invite) -> Vec<Effect> {
    hub.local(
        at,
        LocalCmd::Invite {
            secret: secret.to_string(),
            invite,
            minter: minter.clone(),
        },
    )
}

fn present(hub: &mut Hub, home: &NodeId, who: &NodeId, name: &str, secret: &str) {
    let token = JoinToken {
        cluster: "c1".to_string(),
        home: home.clone(),
        fingerprint: fp("m5"),
        addrs: vec![],
        port: 1,
        secret: secret.to_string(),
    };
    hub.local(
        who,
        LocalCmd::JoinWith {
            token,
            me: member(name, "linux", false),
        },
    );
    hub.linked.push((home.clone(), who.clone()));
    hub.run(home, Input::Linked { peer: who.clone() });
    hub.run(who, Input::Linked { peer: home.clone() });
}

fn state_of(hub: &Hub, home: &NodeId, who: &NodeId) -> Option<MemberState> {
    hub.core(home)
        .state()
        .and_then(|s| s.members.get(who))
        .map(|m| m.state)
}

fn join_requests(hub: &Hub, who: &NodeId) -> usize {
    hub.seen
        .iter()
        .filter(|(_, e)| matches!(e, Effect::JoinRequest { node, .. } if node == who))
        .count()
}

fn refusals(hub: &Hub, to: &NodeId) -> Vec<String> {
    hub.seen
        .iter()
        .filter_map(|(_, e)| match e {
            Effect::Send {
                to: t,
                msg: Msg::Refused { reason, .. },
            } if t == to => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn invite_from_its_node_approves_without_join_request() {
    let (mut hub, m5, _) = pair();
    let w9 = hub.add("w9", "linux", false);
    make(
        &mut hub,
        &m5,
        "inv-1",
        &m5,
        invite_for(&w9, "box9", false, false),
    );
    present(&mut hub, &m5, &w9, "w9", "inv-1");
    assert_eq!(join_requests(&hub, &w9), 0);
    assert_eq!(state_of(&hub, &m5, &w9), Some(MemberState::Active));
    let joined = hub
        .seen
        .iter()
        .find_map(|(_, e)| match e {
            Effect::InviteJoined {
                node,
                name,
                os,
                arch,
                minter,
                minter_name,
            } if *node == w9 => Some((
                name.clone(),
                os.clone(),
                arch.clone(),
                minter.clone(),
                minter_name.clone(),
            )),
            _ => None,
        })
        .expect("invite joined");
    assert_eq!(
        joined,
        (
            "box9".to_string(),
            "linux".to_string(),
            "arm64".to_string(),
            m5.clone(),
            "m5".to_string()
        )
    );
    let worker = hub.core(&w9).state().expect("joined state");
    assert_eq!(worker.home, m5);
    assert!(hub.core(&m5).online().contains(&w9));
}

#[test]
fn invite_sets_name_can_be_master_and_acting() {
    let (mut hub, m5, _) = pair();
    let w9 = hub.add("w9", "linux", false);
    make(
        &mut hub,
        &m5,
        "inv-1",
        &m5,
        invite_for(&w9, "Studio", true, true),
    );
    present(&mut hub, &m5, &w9, "w9", "inv-1");
    let state = hub.core(&m5).state().expect("state");
    let m = &state.members[&w9];
    assert_eq!(m.name, "Studio");
    assert!(m.can_be_master);
    assert!(m.desired.acting);
    assert!(!m.desired.bridge);
    assert!(!m.desired.install);
    assert!(!m.client);
    assert_eq!(m.state, MemberState::Active);
}

#[test]
fn invite_without_choices_keeps_everything_off() {
    let (mut hub, m5, _) = pair();
    let w9 = hub.add("w9", "linux", false);
    make(
        &mut hub,
        &m5,
        "inv-1",
        &m5,
        invite_for(&w9, "box9", false, false),
    );
    present(&mut hub, &m5, &w9, "w9", "inv-1");
    let state = hub.core(&m5).state().expect("state");
    let m = &state.members[&w9];
    assert!(!m.can_be_master);
    assert!(!m.desired.acting && !m.desired.bridge && !m.desired.install);
}

#[test]
fn invite_name_is_cleaned_like_a_joiners() {
    let (mut hub, m5, _) = pair();
    let w9 = hub.add("w9", "linux", false);
    make(
        &mut hub,
        &m5,
        "inv-1",
        &m5,
        invite_for(&w9, "bad\u{7}\nname", false, false),
    );
    present(&mut hub, &m5, &w9, "w9", "inv-1");
    let state = hub.core(&m5).state().expect("state");
    let name = &state.members[&w9].name;
    assert!(!name.contains('\u{7}') && !name.contains('\n'), "{name:?}");
    assert!(!name.is_empty());
}

#[test]
fn invite_from_another_node_is_refused_and_spent() {
    let (mut hub, m5, _) = pair();
    let w9 = hub.add("w9", "linux", false);
    let w8 = hub.add("w8", "linux", false);
    make(
        &mut hub,
        &m5,
        "inv-1",
        &m5,
        invite_for(&w9, "box9", false, false),
    );
    present(&mut hub, &m5, &w8, "w8", "inv-1");
    assert_eq!(
        refusals(&hub, &w8),
        vec!["this token was made for another machine".to_string()]
    );
    assert_ne!(state_of(&hub, &m5, &w8), Some(MemberState::Active));
    assert_eq!(join_requests(&hub, &w8), 0);
    present(&mut hub, &m5, &w9, "w9", "inv-1");
    assert_eq!(refusals(&hub, &w9), vec!["bad token".to_string()]);
    assert_eq!(state_of(&hub, &m5, &w9), None);
    assert!(!hub.core(&m5).pin_set().1);
}

#[test]
fn expired_invite_is_refused() {
    let (mut hub, m5, _) = pair();
    let w9 = hub.add("w9", "linux", false);
    make(
        &mut hub,
        &m5,
        "inv-1",
        &m5,
        invite_for(&w9, "box9", false, false),
    );
    hub.tick(&m5, TOKEN_TTL_MS as u64 + 1);
    assert!(!hub.core(&m5).pin_set().1);
    present(&mut hub, &m5, &w9, "w9", "inv-1");
    assert_eq!(refusals(&hub, &w9), vec!["bad token".to_string()]);
    assert_eq!(state_of(&hub, &m5, &w9), None);
}

#[test]
fn restart_forgets_invites() {
    let (mut hub, m5, _) = pair();
    let w9 = hub.add("w9", "linux", false);
    make(
        &mut hub,
        &m5,
        "inv-1",
        &m5,
        invite_for(&w9, "box9", false, false),
    );
    let saved = hub.core(&m5).state().cloned();
    let core = crate::cluster::core::NodeCore::new(
        m5.clone(),
        crate::cluster::testkit::versions("macos"),
        saved,
        true,
    )
    .with_boot_id("boot-again".to_string());
    hub.nodes.insert(m5.clone(), core);
    present(&mut hub, &m5, &w9, "w9", "inv-1");
    assert_eq!(refusals(&hub, &w9), vec!["bad token".to_string()]);
    assert_eq!(state_of(&hub, &m5, &w9), None);
}

#[test]
fn plain_token_still_makes_join_request() {
    let (mut hub, m5, _) = pair();
    let w9 = hub.add("w9", "linux", false);
    hub.local(
        &m5,
        LocalCmd::IssueToken {
            secret: "plain".to_string(),
            minter: m5.clone(),
        },
    );
    present(&mut hub, &m5, &w9, "w9", "plain");
    assert_eq!(join_requests(&hub, &w9), 1);
    assert_eq!(state_of(&hub, &m5, &w9), Some(MemberState::Pending));
    assert!(!hub
        .seen
        .iter()
        .any(|(_, e)| matches!(e, Effect::InviteJoined { .. })));
}

#[test]
fn fifth_invite_drops_the_oldest() {
    let (mut hub, m5, _) = pair();
    let w9 = hub.add("w9", "linux", false);
    for n in 0..=MAX_TOKENS_PER_MINTER {
        make(
            &mut hub,
            &m5,
            &format!("inv-{n}"),
            &m5,
            invite_for(&w9, "box9", false, false),
        );
        hub.tick(&m5, 1_000);
    }
    present(&mut hub, &m5, &w9, "w9", "inv-0");
    assert_eq!(refusals(&hub, &w9), vec!["bad token".to_string()]);
    assert_eq!(state_of(&hub, &m5, &w9), None);
    hub.linked.clear();
    present(&mut hub, &m5, &w9, "w9", "inv-4");
    assert_eq!(state_of(&hub, &m5, &w9), Some(MemberState::Active));
}

#[test]
fn only_home_takes_invites() {
    let (mut hub, _, w1) = pair();
    let other = hub.add("w9", "linux", false);
    let effects = make(
        &mut hub,
        &w1,
        "inv-1",
        &w1,
        invite_for(&other, "box9", false, false),
    );
    assert!(answers(&effects)
        .iter()
        .any(|r| matches!(r, Err(e) if e.code == "forbidden")));
    assert!(!hub.core(&w1).pin_set().1);
}

#[test]
fn only_a_member_can_be_named_as_the_maker_of_an_invite() {
    let (mut hub, m5, _) = pair();
    let w9 = hub.add("w9", "linux", false);
    let effects = make(
        &mut hub,
        &m5,
        "inv-1",
        &id("nobody"),
        invite_for(&w9, "box9", false, false),
    );
    assert!(answers(&effects)
        .iter()
        .any(|r| matches!(r, Err(e) if e.code == "forbidden")));
    assert!(!hub.core(&m5).pin_set().1);
}

#[test]
fn an_invite_answers_with_its_expiry_and_opens_the_join_window() {
    let (mut hub, m5, _) = pair();
    let w9 = hub.add("w9", "linux", false);
    hub.tick(&m5, 2_000);
    let effects = make(
        &mut hub,
        &m5,
        "inv-1",
        &m5,
        invite_for(&w9, "box9", false, false),
    );
    let ok = answers(&effects)
        .into_iter()
        .find_map(|r| r.as_ref().ok())
        .expect("answer");
    assert_eq!(ok["expiresMs"], 2_000 + TOKEN_TTL_MS);
    assert!(!ok.to_string().contains("inv-1"));
    assert!(hub.core(&m5).pin_set().1);
}

#[test]
fn the_secret_never_leaves_the_homes_effects() {
    let (mut hub, m5, _) = pair();
    let w9 = hub.add("w9", "linux", false);
    let secret = "hush-invite-secret";
    let made = make(
        &mut hub,
        &m5,
        secret,
        &m5,
        invite_for(&w9, "box9", false, true),
    );
    assert!(!format!("{made:?}").contains(secret));
    present(&mut hub, &m5, &w9, "w9", secret);
    let home_effects: Vec<&Effect> = hub.effects_of("m5");
    assert!(!home_effects.is_empty());
    for e in home_effects {
        assert!(!format!("{e:?}").contains(secret), "{e:?}");
    }
    assert!(!hub.core(&m5).status().to_string().contains(secret));
}

fn invite_call(
    hub: &mut Hub,
    home: &NodeId,
    from: &NodeId,
    args: serde_json::Value,
) -> Vec<Effect> {
    let term = hub.core(home).highest_term().cloned().expect("term");
    hub.nodes.get_mut(home).expect("node").step(Input::Msg {
        from: from.clone(),
        msg: Msg::Call {
            id: 3,
            op: "cluster.invite".to_string(),
            args: args.into(),
            origin: Origin {
                node: from.clone(),
                tier: OriginTier::App,
            },
            term,
        },
    })
}

#[test]
fn a_forwarded_invite_names_the_calling_member_whatever_its_args_say() {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, false);
    let args = json!({"node": id("w9"), "name": "box9", "minter": m5});
    let effects = invite_call(&mut hub, &m5, &m1, args);
    let runs: Vec<_> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::RunLocal { op, args, .. } => Some((op.clone(), (**args).clone())),
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 1, "{effects:?}");
    assert_eq!(runs[0].0, "cluster.invite");
    assert_eq!(runs[0].1["minter"], json!(m1));
    assert_eq!(runs[0].1["node"], json!(id("w9")));
    assert_eq!(runs[0].1["name"], "box9");
}

#[test]
fn a_forwarded_invite_is_never_relayed_on() {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    let m2 = hub.add("m2", "macos", true);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, false);
    hub.join_as("m5", "m2", "macos", true, false);
    hub.link(&m1, &m2);
    let _ = &m5;
    let args = json!({"node": id("w9"), "name": "box9"});
    let effects = invite_call(&mut hub, &m1, &m2, args);
    assert!(!effects.iter().any(|e| matches!(e, Effect::RunLocal { .. })));
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Send {
            msg: Msg::Reply { err: Some(_), .. },
            ..
        }
    )));
}
