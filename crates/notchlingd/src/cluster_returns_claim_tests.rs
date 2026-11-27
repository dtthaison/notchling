use crate::cluster_fence_keeper_kit::{claim, node, S1};
use crate::cluster_returns_kit::{kit, seed_after_promote};
use notchling_engine::cluster::fence::Gate;
use notchling_engine::cluster::state::Command;
use notchling_engine::cluster::wire::OriginTier;

#[test]
fn handover_target_claims_a_claimed_session_before_starting_it() {
    let k = kit("claim-first", seed_after_promote(&[(S1, 1)]));
    k.write_transcript(S1, 40);
    let rows = k.mover.start_here(&[S1.to_string()]);
    assert!(rows[0].1.is_ok(), "{rows:?}");
    assert_eq!(
        k.logged(),
        vec![format!("record claim {S1} 2"), format!("start {S1}")]
    );
    let held = k.state().claims[S1].clone();
    assert_eq!(held.node, node("m5"));
    assert_eq!(held.n, 2);
    assert_eq!(held.base_len, 40);
    assert_eq!(held.base_set, "set");
    assert!(!held.touched);
    assert_eq!(held.term, k.state().term);
    let records = k.link.records.lock().unwrap();
    assert_eq!(records[0].1, OriginTier::Node);
}

#[test]
fn a_failed_claim_record_starts_nothing() {
    let k = kit("claim-fails", seed_after_promote(&[(S1, 1)]));
    *k.link.fail.lock().unwrap() = Some("the primary is away".to_string());
    let rows = k.mover.start_here(&[S1.to_string()]);
    let why = rows[0].1.clone().unwrap_err();
    assert!(why.contains("claim wasn't recorded"), "{why}");
    assert!(why.contains("the primary is away"), "{why}");
    assert!(k.logged().is_empty());
    let rows = k.mover.restart(&[S1.to_string()], &[]);
    assert!(rows[0].1.is_err());
    assert!(k.logged().is_empty());
}

#[test]
fn a_competing_newer_claim_that_wins_first_keeps_the_loser_from_starting() {
    let k = kit("claim-rival", seed_after_promote(&[(S1, 1)]));
    *k.link.rival.lock().unwrap() = Some(Command::Claim {
        claim: claim(S1, "m1", 9),
    });
    let rows = k.mover.start_here(&[S1.to_string()]);
    let why = rows[0].1.clone().unwrap_err();
    assert!(why.contains("newer claim"), "{why}");
    assert_eq!(k.logged(), vec![format!("record claim {S1} 2")]);
    assert_eq!(k.state().claims[S1].node, node("m1"));
    k.w.keeper.tick();
    assert_eq!(k.w.gate(S1), Some(Gate::Moved { to: "m1".into() }));
}

#[test]
fn a_session_nobody_claimed_starts_without_a_claim() {
    let k = kit("claim-none", seed_after_promote(&[]));
    let rows = k.mover.start_here(&[S1.to_string()]);
    assert!(rows[0].1.is_ok());
    assert_eq!(k.logged(), vec![format!("start {S1}")]);
}

#[test]
fn a_session_claimed_here_starts_without_a_new_claim() {
    let mut seed = seed_after_promote(&[]);
    seed.claims = vec![claim(S1, "m5", 3)];
    let k = kit("claim-mine", seed);
    let rows = k.mover.start_here(&[S1.to_string()]);
    assert!(rows[0].1.is_ok());
    assert_eq!(k.logged(), vec![format!("start {S1}")]);
}

#[test]
fn restart_claims_a_session_another_machine_holds() {
    let k = kit("claim-restart", seed_after_promote(&[(S1, 4)]));
    let rows = k.mover.restart(&[S1.to_string()], &[]);
    assert!(rows[0].1.is_ok());
    assert_eq!(
        k.logged(),
        vec![format!("record claim {S1} 5"), format!("start {S1}")]
    );
}

#[test]
fn take_back_after_a_promote_leaves_no_moved_gate() {
    let k = kit("claim-keeper", seed_after_promote(&[(S1, 1)]));
    k.w.keeper.tick();
    assert_eq!(k.w.gate(S1), Some(Gate::Moved { to: "m1".into() }));
    let rows = k.mover.start_here(&[S1.to_string()]);
    assert!(rows[0].1.is_ok());
    k.w.keeper.tick();
    assert_eq!(k.w.gate(S1), None);
    assert!(k.w.acts.stops.lock().unwrap().is_empty());
    assert!(matches!(k.recorded()[0], Command::Claim { .. }));
}

#[test]
fn an_unreadable_cluster_record_claims_nothing_and_says_so() {
    let k = kit("claim-unreadable", seed_after_promote(&[(S1, 1)]));
    let main = k.state().main;
    let db =
        k.w.cluster
            .join(notchling_engine::cluster::persist::CLUSTER_DB);
    std::fs::write(&db, b"not a database").unwrap();
    let why = k.mover.claim_here(S1, &main).unwrap_err();
    assert!(why.contains("can't be read"), "{why}");
    assert!(k.logged().is_empty());
}
