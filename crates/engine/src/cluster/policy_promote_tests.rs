use super::id::Role;
use super::members::MemberState;
use super::policy::{check, SenderFacts};
use super::testkit::id;
use super::wire::{Origin, OriginTier};

fn facts(name: &str, role: Role, client: bool, state: MemberState) -> SenderFacts {
    SenderFacts {
        id: id(name),
        role,
        client,
        state,
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
fn a_standby_without_an_app_may_forward_a_promote_at_the_app_tier_only() {
    let standby = facts("m2", Role::Standby, false, MemberState::Active);
    let rule = check("cluster.promote", &standby, &origin("m2", OriginTier::App)).expect("rule");
    assert!(!rule.home_only, "it goes to the machine being promoted");
    assert_eq!(rule.id_field, None);
    for tier in [OriginTier::Agent, OriginTier::Node] {
        assert!(
            check("cluster.promote", &standby, &origin("m2", tier)).is_err(),
            "{tier:?}"
        );
    }
}

#[test]
fn a_worker_still_cannot_send_a_promote_at_any_tier() {
    let worker = facts("w1", Role::Worker, false, MemberState::Active);
    for tier in [OriginTier::App, OriginTier::Node, OriginTier::Agent] {
        assert!(
            check("cluster.promote", &worker, &origin("w1", tier)).is_err(),
            "{tier:?}"
        );
    }
}
