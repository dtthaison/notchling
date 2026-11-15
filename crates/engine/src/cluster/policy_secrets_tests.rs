use super::id::Role;
use super::members::MemberState;
use super::policy::{check, Denied, SenderFacts};
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

fn vault_home(name: &str) -> SenderFacts {
    let mut f = facts(name, Role::Home, true, MemberState::Active);
    f.is_vault = true;
    f
}

#[test]
fn only_the_vault_node_while_home_delivers_and_revokes() {
    let vault = vault_home("m5");
    for op in ["grant.deliver", "grant.revoke"] {
        assert!(
            check(op, &vault, &origin("m5", OriginTier::Node)).is_ok(),
            "{op}"
        );
    }
    let promoted = facts("m1", Role::Home, true, MemberState::Active);
    let mut standby_vault = facts("m5", Role::Standby, true, MemberState::Active);
    standby_vault.is_vault = true;
    let worker = facts("w1", Role::Worker, false, MemberState::Active);
    for (who, sender) in [("m1", &promoted), ("m5", &standby_vault), ("w1", &worker)] {
        for op in ["grant.deliver", "grant.revoke"] {
            assert_eq!(
                check(op, sender, &origin(who, OriginTier::Node)).err(),
                Some(Denied::NotAllowed),
                "{who} {op}"
            );
        }
    }
    assert!(check("grant.deliver", &vault, &origin("m5", OriginTier::Agent)).is_err());
    assert!(check("grant.deliver", &vault, &origin("m5", OriginTier::App)).is_err());
}

#[test]
fn a_member_asks_the_vault_node_for_a_grant_as_an_agent_only() {
    let worker = facts("w1", Role::Worker, false, MemberState::Active);
    let rule = check("grant.request", &worker, &origin("w1", OriginTier::Agent)).expect("rule");
    assert!(rule.vault_only);
    for tier in [OriginTier::Node, OriginTier::App] {
        assert!(
            check("grant.request", &worker, &origin("w1", tier)).is_err(),
            "{tier:?}"
        );
    }
    let removed = facts("w1", Role::Worker, false, MemberState::Removed);
    assert_eq!(
        check("grant.request", &removed, &origin("w1", OriginTier::Agent)).err(),
        Some(Denied::Removed)
    );
}

#[test]
fn a_client_relays_vault_ops_to_the_vault_node_and_a_worker_cannot() {
    let client = facts("m1", Role::Standby, true, MemberState::Active);
    for op in ["vault.list", "vault.save", "vault.remove"] {
        let rule = check(op, &client, &origin("m1", OriginTier::App)).expect(op);
        assert!(rule.vault_only, "{op}");
        assert_eq!(
            check(op, &client, &origin("m1", OriginTier::Agent)).err(),
            Some(Denied::NotAllowed),
            "{op}"
        );
        let worker = facts("w1", Role::Worker, false, MemberState::Active);
        assert!(
            check(op, &worker, &origin("w1", OriginTier::App)).is_err(),
            "{op}"
        );
    }
}

#[test]
fn the_allowlist_is_set_on_the_home_by_a_client() {
    let client = facts("m1", Role::Standby, true, MemberState::Active);
    let rule = check("cluster.allow.set", &client, &origin("m1", OriginTier::App)).expect("rule");
    assert!(rule.home_only);
    let worker = facts("w1", Role::Worker, false, MemberState::Active);
    assert!(check("cluster.allow.set", &worker, &origin("w1", OriginTier::App)).is_err());
}
