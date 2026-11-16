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
fn every_member_may_send_its_own_inbox_items_at_the_node_tier() {
    for (name, role, client) in [
        ("w1", Role::Worker, false),
        ("m1", Role::Standby, true),
        ("m5", Role::Home, true),
    ] {
        let sender = facts(name, role, client);
        let rule = check("node.inbox", &sender, &origin(name, OriginTier::Node)).expect(name);
        assert_eq!(rule.id_field, None);
        assert!(!rule.home_only);
    }
}

#[test]
fn node_inbox_names_its_own_sender_only() {
    let worker = facts("w1", Role::Worker, false);
    assert_eq!(
        check("node.inbox", &worker, &origin("w2", OriginTier::Node)).err(),
        Some(Denied::NotAllowed)
    );
}

#[test]
fn node_inbox_is_not_an_app_or_agent_op() {
    let worker = facts("w1", Role::Worker, false);
    for tier in [OriginTier::App, OriginTier::Agent] {
        assert!(check("node.inbox", &worker, &origin("w1", tier)).is_err());
    }
}

#[test]
fn a_client_or_the_home_may_answer_and_list_at_the_app_tier() {
    for op in ["inbox.answer", "inbox.list"] {
        for (name, role) in [("m1", Role::Standby), ("m5", Role::Home)] {
            let sender = facts(name, role, true);
            let rule = check(op, &sender, &origin(name, OriginTier::App)).expect(op);
            assert_eq!(rule.id_field, None, "{op}");
        }
    }
}

#[test]
fn a_worker_may_not_answer_or_list_other_nodes_inboxes() {
    let worker = facts("w1", Role::Worker, false);
    for op in ["inbox.answer", "inbox.list"] {
        for tier in [OriginTier::App, OriginTier::Node, OriginTier::Agent] {
            assert!(check(op, &worker, &origin("w1", tier)).is_err(), "{op}");
        }
    }
}

#[test]
fn a_client_may_not_answer_at_the_node_or_agent_tier() {
    let client = facts("m1", Role::Standby, true);
    for op in ["inbox.answer", "inbox.list"] {
        for tier in [OriginTier::Node, OriginTier::Agent] {
            assert!(check(op, &client, &origin("m1", tier)).is_err(), "{op}");
        }
    }
}
