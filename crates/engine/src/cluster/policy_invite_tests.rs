use super::id::Role;
use super::members::MemberState;
use super::policy::{check, Denied, SenderFacts};
use super::testkit::id;
use super::wire::{Origin, OriginTier};

fn facts(name: &str, role: Role, client: bool) -> SenderFacts {
    SenderFacts {
        id: id(name),
        role,
        client,
        state: MemberState::Active,
        is_home: role == Role::Home,
        is_vault: false,
    }
}

fn origin(name: &str, tier: OriginTier) -> Origin {
    Origin {
        node: id(name),
        tier,
    }
}

#[test]
fn client_may_forward_invite_at_app() {
    let client = facts("m1", Role::Standby, true);
    let rule = check("cluster.invite", &client, &origin("m1", OriginTier::App)).expect("rule");
    assert!(rule.home_only);
    let home = facts("m5", Role::Home, true);
    assert!(check("cluster.invite", &home, &origin("m5", OriginTier::App)).is_ok());
}

#[test]
fn standby_without_app_cannot_forward_invite() {
    let standby = facts("m2", Role::Standby, false);
    for tier in [OriginTier::App, OriginTier::Agent, OriginTier::Node] {
        assert!(
            check("cluster.invite", &standby, &origin("m2", tier)).is_err(),
            "{tier:?}"
        );
    }
}

#[test]
fn worker_cannot_forward_invite_at_any_tier() {
    let worker = facts("w1", Role::Worker, false);
    for tier in [OriginTier::App, OriginTier::Agent, OriginTier::Node] {
        assert!(
            check("cluster.invite", &worker, &origin("w1", tier)).is_err(),
            "{tier:?}"
        );
    }
}

#[test]
fn agent_origin_invite_is_refused() {
    let client = facts("m1", Role::Standby, true);
    let home = facts("m5", Role::Home, true);
    assert_eq!(
        check("cluster.invite", &client, &origin("m1", OriginTier::Agent)).err(),
        Some(Denied::NotAllowed)
    );
    assert!(check("cluster.invite", &home, &origin("m5", OriginTier::Agent)).is_err());
    assert!(check("cluster.invite", &client, &origin("m1", OriginTier::Node)).is_err());
}

#[test]
fn invite_cannot_borrow_another_nodes_origin() {
    let client = facts("m1", Role::Standby, true);
    assert_eq!(
        check("cluster.invite", &client, &origin("m5", OriginTier::App)).err(),
        Some(Denied::NotAllowed)
    );
}
