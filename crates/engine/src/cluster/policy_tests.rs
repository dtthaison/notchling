use super::id::Role;
use super::members::MemberState;
use super::policy::{check, cluster_may_set, Denied, SenderFacts, RULES};
use super::testkit::id;
use super::wire::{Origin, OriginTier};

fn facts(name: &str, role: Role, client: bool, state: MemberState) -> SenderFacts {
    SenderFacts {
        id: id(name),
        role,
        client,
        state,
        is_home: role == Role::Home,
    }
}

fn origin(name: &str, tier: OriginTier) -> Origin {
    Origin {
        node: id(name),
        tier,
    }
}

#[test]
fn home_may_start_sessions() {
    let home = facts("m5", Role::Home, true, MemberState::Active);
    for op in [
        "session.new",
        "codex.send",
        "agy.open",
        "shell.run",
        "workspace.prepare",
    ] {
        assert!(
            check(op, &home, &origin("m5", OriginTier::Node)).is_ok(),
            "{op}"
        );
    }
    assert!(check("delegate.run", &home, &origin("m5", OriginTier::Agent)).is_ok());
}

#[test]
fn worker_may_not_dispatch() {
    let worker = facts("w1", Role::Worker, false, MemberState::Active);
    assert_eq!(
        check("session.new", &worker, &origin("w1", OriginTier::Node)).err(),
        Some(Denied::NotAllowed)
    );
    assert_eq!(
        check("cluster.dispatch", &worker, &origin("w1", OriginTier::Node)).err(),
        Some(Denied::NotAllowed)
    );
}

#[test]
fn client_app_origin_may_reply() {
    let client = facts("m1", Role::Standby, true, MemberState::Active);
    let rule = check("session.reply", &client, &origin("m1", OriginTier::App)).expect("rule");
    assert_eq!(rule.id_field, Some("id"));
    let dispatch =
        check("cluster.dispatch", &client, &origin("m1", OriginTier::App)).expect("rule");
    assert!(dispatch.home_only);
}

#[test]
fn non_client_claiming_app_is_refused() {
    let worker = facts("w1", Role::Worker, false, MemberState::Active);
    assert_eq!(
        check("session.reply", &worker, &origin("w1", OriginTier::App)).err(),
        Some(Denied::TierTooHigh)
    );
    assert_eq!(
        check("node.sessions", &worker, &origin("w1", OriginTier::Agent)).err(),
        Some(Denied::TierTooHigh)
    );
}

#[test]
fn a_call_must_come_from_its_own_origin() {
    let worker = facts("w1", Role::Worker, false, MemberState::Active);
    assert_eq!(
        check("node.load", &worker, &origin("w2", OriginTier::Node)).err(),
        Some(Denied::NotAllowed)
    );
    assert!(check("node.load", &worker, &origin("w1", OriginTier::Node)).is_ok());
}

#[test]
fn removed_member_is_refused() {
    let gone = facts("w1", Role::Worker, false, MemberState::Removed);
    assert_eq!(
        check("node.load", &gone, &origin("w1", OriginTier::Node)).err(),
        Some(Denied::Removed)
    );
    let pending = facts("w2", Role::Worker, false, MemberState::Pending);
    assert_eq!(
        check("node.load", &pending, &origin("w2", OriginTier::Node)).err(),
        Some(Denied::NotAMember)
    );
}

#[test]
fn unknown_op_is_not_allowed() {
    let home = facts("m5", Role::Home, true, MemberState::Active);
    assert_eq!(
        check("vault.get", &home, &origin("m5", OriginTier::Node)).err(),
        Some(Denied::NotAllowed)
    );
}

#[test]
fn member_may_ask_for_a_join_token_and_a_delegate_dispatch() {
    let worker = facts("w1", Role::Worker, false, MemberState::Active);
    let rule = check(
        "cluster.join.token",
        &worker,
        &origin("w1", OriginTier::Agent),
    )
    .expect("rule");
    assert!(rule.home_only);
    assert!(check(
        "cluster.dispatch",
        &worker,
        &origin("w1", OriginTier::Agent)
    )
    .is_ok());
}

#[test]
fn cluster_never_sets_vault_or_accounts() {
    assert!(cluster_may_set("acting"));
    assert!(cluster_may_set("bridge"));
    assert!(cluster_may_set("install"));
    assert!(!cluster_may_set("vault"));
    assert!(!cluster_may_set("accounts"));
    assert!(!cluster_may_set(""));
}

#[test]
fn every_rule_names_a_known_op() {
    const NODE_ONLY: &[&str] = &[
        "shell.run",
        "shell.stop",
        "delegate.run",
        "workspace.prepare",
        "workspace.return",
        "node.switch.set",
        "node.put",
        "node.put.end",
        "node.tail",
        "node.sessions",
        "node.versions",
        "node.load",
        "cluster.dispatch",
        "cluster.member.set",
        "cluster.remove",
        "cluster.join.token",
    ];
    let wire = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../local-api/src/wire.rs"
    ))
    .expect("local-api wire.rs");
    for rule in RULES {
        let quoted = format!("\"{}\"", rule.op);
        assert!(
            NODE_ONLY.contains(&rule.op) || wire.contains(&quoted),
            "{} is neither a local-api op nor a node-only one",
            rule.op
        );
    }
}
