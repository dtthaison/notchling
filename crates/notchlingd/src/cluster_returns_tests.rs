use crate::cluster_fence_keeper_kit::{claim, node, S1};
use crate::cluster_returns_kit::{kit, seed_after_promote, Kit};
use notchling_engine::cluster::state::Command;
use notchling_engine::cluster::wire::OriginTier;
use notchling_engine::inbox::{ActionId, Kind, State};
use serde_json::json;
use std::path::PathBuf;

fn with_copy(tag: &str) -> (Kit, PathBuf) {
    let k = kit(tag, seed_after_promote(&[(S1, 1)]));
    let path = k.write_transcript(S1, 100);
    k.side_copy(S1, &path);
    (k, path)
}

fn aside_path(k: &Kit, name: &str) -> PathBuf {
    k.dir
        .join("cluster")
        .join("copies")
        .join(S1)
        .join("set")
        .join(name)
}

#[test]
fn take_back_runs_the_handover_with_aside() {
    let (k, path) = with_copy("tb-run");
    let name = path.file_name().unwrap().to_str().unwrap().to_string();
    let answer = k.returns.take_back().expect("the handover");
    assert_eq!(answer, json!({"started": true}));
    let moved = aside_path(&k, &name);
    assert!(moved.exists());
    assert!(!path.exists());
    assert_eq!(std::fs::read(&moved).unwrap().len(), 100);
    let calls = k.link.app_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, node("m1"));
    assert_eq!(calls[0].1, "cluster.move_home");
    assert_eq!(calls[0].2, json!({"to": node("m5")}));
    drop(calls);
    assert_eq!(
        k.logged(),
        vec![
            format!("record side-copy {S1}"),
            "call cluster.move_home".to_string()
        ]
    );
    let copy = &k.state().side_copies[0];
    assert_eq!(copy.path.as_deref(), Some(moved.to_str().unwrap()));
    assert!(!copy.hidden);
}

#[test]
fn take_back_moves_a_hidden_copy_and_keeps_it_hidden() {
    let (k, path) = with_copy("tb-hidden");
    k.returns.hide(S1, &node("m5")).unwrap();
    k.returns.take_back().unwrap();
    assert!(!path.exists());
    let copy = &k.state().side_copies[0];
    assert!(copy.hidden);
    assert!(copy.path.as_deref().unwrap().contains("copies"));
}

#[test]
fn take_back_never_overwrites_a_copy() {
    let (k, path) = with_copy("tb-clobber");
    let name = path.file_name().unwrap().to_str().unwrap().to_string();
    let taken = aside_path(&k, &name);
    std::fs::create_dir_all(taken.parent().unwrap()).unwrap();
    std::fs::write(&taken, b"older").unwrap();
    let why = k.returns.take_back().unwrap_err().message;
    assert!(why.contains("already holds a copy"), "{why}");
    assert_eq!(std::fs::read(&taken).unwrap(), b"older");
    assert!(path.exists());
    assert!(k.link.app_calls.lock().unwrap().is_empty());
}

#[test]
fn take_back_stops_when_the_handover_is_refused() {
    let (k, path) = with_copy("tb-refused");
    *k.link.app_answer.lock().unwrap() = Err(notchling_engine::cluster::wire::WireError::new(
        "failed",
        "m1 can't move now",
    ));
    let why = k.returns.take_back().unwrap_err().message;
    assert_eq!(why, "m1 can't move now");
    assert!(!path.exists(), "files already moved stay moved");
    assert!(k.state().side_copies[0]
        .path
        .as_deref()
        .unwrap()
        .contains("copies"));
}

#[test]
fn take_back_on_the_primary_is_refused() {
    let mut seed = seed_after_promote(&[]);
    seed.home = "m5";
    let k = kit("tb-primary", seed);
    let why = k.returns.take_back().unwrap_err().message;
    assert_eq!(why, "This machine is already the primary");
    assert!(k.link.app_calls.lock().unwrap().is_empty());
}

#[test]
fn take_back_runs_again_after_a_record_failed() {
    let (k, path) = with_copy("tb-rerun");
    *k.link.fail.lock().unwrap() = Some("offline".to_string());
    assert!(k.returns.take_back().is_err());
    assert!(!path.exists());
    assert!(k.link.app_calls.lock().unwrap().is_empty());
    *k.link.fail.lock().unwrap() = None;
    k.returns.take_back().expect("the second try");
    assert!(k.state().side_copies[0]
        .path
        .as_deref()
        .unwrap()
        .contains("copies"));
    assert_eq!(k.link.app_calls.lock().unwrap().len(), 1);
}

#[test]
fn take_back_from_the_item_runs_the_handover() {
    let (k, _path) = with_copy("tb-item");
    k.was_primary(1);
    k.returns.tick();
    let outcome =
        k.w.inbox
            .decide("primary-back:2", ActionId::TakeBack, "mac")
            .unwrap();
    assert!(outcome.answered);
    crate::cluster_testkit::eventually("the handover call", || {
        !k.link.app_calls.lock().unwrap().is_empty()
    });
}

#[test]
fn phone_take_back_needs_acting() {
    let (k, path) = with_copy("tb-acting");
    k.was_primary(1);
    k.returns.tick();
    k.acting.set(false);
    let outcome =
        k.w.inbox
            .decide("primary-back:2", ActionId::TakeBack, "phone")
            .unwrap();
    assert!(!outcome.answered);
    assert_eq!(k.item("primary-back:2").unwrap().state, State::Open);
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(path.exists());
    assert!(k.link.app_calls.lock().unwrap().is_empty());
}

#[test]
fn a_failed_take_back_from_the_item_says_why() {
    let (k, _path) = with_copy("tb-item-fails");
    k.was_primary(1);
    k.returns.tick();
    *k.link.fail.lock().unwrap() = Some("offline".to_string());
    k.w.inbox
        .decide("primary-back:2", ActionId::TakeBack, "mac")
        .unwrap();
    crate::cluster_testkit::eventually("the failure item", || {
        k.item("take-back-failed:2").is_some()
    });
    assert_eq!(
        k.item("take-back-failed:2").unwrap().kind,
        Kind::ActionFailed
    );
}

#[test]
fn hide_keeps_the_side_copy_on_disk() {
    let (k, path) = with_copy("hide");
    k.returns.hide(S1, &node("m5")).unwrap();
    assert!(path.exists());
    assert_eq!(std::fs::read(&path).unwrap().len(), 100);
    let copies = k.state().side_copies;
    assert_eq!(copies.len(), 1);
    assert!(copies[0].hidden);
    let records = k.link.records.lock().unwrap();
    assert!(matches!(records[0].0, Command::HideSideCopy { .. }));
    assert_eq!(records[0].1, OriginTier::App);
}

#[test]
fn hiding_a_copy_that_isnt_there_fails() {
    let k = kit("hide-none", seed_after_promote(&[]));
    assert!(k.returns.hide(S1, &node("m5")).is_err());
}

#[test]
fn continue_here_claims_the_session() {
    let k = kit("continue", seed_after_promote(&[(S1, 1)]));
    k.write_transcript(S1, 30);
    k.returns.add_waiting(S1, &node("m1")).unwrap();
    k.returns.tick();
    assert_eq!(
        k.item(&format!("session-waits:{S1}")).unwrap().state,
        State::Open
    );
    let answer = k.returns.continue_here(S1).unwrap();
    assert_eq!(answer, json!({"id": S1, "claimed": true}));
    let held = k.state().claims[S1].clone();
    assert_eq!(held.node, node("m5"));
    assert_eq!(held.n, 2);
    assert_eq!(held.base_len, 30);
    assert!(!held.touched);
    assert!(k.waiting.list().is_empty());
    assert_eq!(
        k.item(&format!("session-waits:{S1}")).unwrap().state,
        State::Answered
    );
}

#[test]
fn continue_here_without_the_home_fails_and_keeps_waiting() {
    let k = kit("continue-fails", seed_after_promote(&[(S1, 1)]));
    k.returns.add_waiting(S1, &node("m1")).unwrap();
    *k.link.fail.lock().unwrap() = Some("no route to the primary".to_string());
    let why = k.returns.continue_here(S1).unwrap_err().message;
    assert!(why.contains("no route to the primary"), "{why}");
    assert_eq!(k.waiting.list().len(), 1);
    assert_eq!(k.state().claims[S1].node, node("m1"));
}

#[test]
fn continue_here_for_a_session_that_isnt_waiting_fails() {
    let k = kit("continue-none", seed_after_promote(&[(S1, 1)]));
    let why = k.returns.continue_here(S1).unwrap_err().message;
    assert_eq!(why, "That session isn't waiting");
    assert!(k.recorded().is_empty());
}

#[test]
fn continue_from_the_item_needs_acting_and_then_claims() {
    let k = kit("continue-item", seed_after_promote(&[(S1, 1)]));
    k.returns.add_waiting(S1, &node("m1")).unwrap();
    k.returns.tick();
    let id = format!("session-waits:{S1}");
    k.acting.set(false);
    let outcome =
        k.w.inbox
            .decide(&id, ActionId::ContinueHere, "phone")
            .unwrap();
    assert!(!outcome.answered);
    assert_eq!(k.waiting.list().len(), 1);
    k.acting.set(true);
    let outcome =
        k.w.inbox
            .decide(&id, ActionId::ContinueHere, "phone")
            .unwrap();
    assert!(outcome.answered);
    crate::cluster_testkit::eventually("the claim", || k.waiting.list().is_empty());
    assert_eq!(k.state().claims[S1].node, node("m5"));
}

#[test]
fn a_failed_continue_from_the_item_raises_a_new_one() {
    let k = kit("continue-retry", seed_after_promote(&[(S1, 1)]));
    k.returns.add_waiting(S1, &node("m1")).unwrap();
    k.returns.tick();
    *k.link.fail.lock().unwrap() = Some("offline".to_string());
    let id = format!("session-waits:{S1}");
    k.w.inbox
        .decide(&id, ActionId::ContinueHere, "mac")
        .unwrap();
    crate::cluster_testkit::eventually("the failure", || {
        k.item(&format!("continue-here-failed:{S1}")).is_some()
    });
    k.returns.tick();
    let again = k.item(&format!("{id}:1")).expect("a fresh item");
    assert_eq!(again.state, State::Open);
}

#[test]
fn wait_for_closes_the_item_and_keeps_waiting() {
    let k = kit("wait-for", seed_after_promote(&[(S1, 1)]));
    k.rows
        .0
        .lock()
        .unwrap()
        .push(crate::cluster_returns_kit::agent_row(
            S1,
            "Fix the build",
            notchling_engine::sessions::AgentGroup::Completed,
        ));
    k.returns.add_waiting(S1, &node("m1")).unwrap();
    k.returns.tick();
    let id = format!("session-waits:{S1}");
    let item = k.item(&id).unwrap();
    assert_eq!(item.kind, Kind::SessionWaits);
    assert_eq!(
        item.title,
        "Fix the build waits for m1, which has newer work in it"
    );
    let labels: Vec<&str> = item.actions.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(labels, vec!["Wait for m1", "Continue From This Copy"]);
    let outcome = k.w.inbox.decide(&id, ActionId::WaitFor, "mac").unwrap();
    assert!(outcome.answered);
    assert_eq!(k.waiting.list().len(), 1);
    k.returns.tick();
    k.returns.tick();
    let items =
        k.w.inbox
            .items()
            .into_iter()
            .filter(|i| i.kind == Kind::SessionWaits)
            .count();
    assert_eq!(items, 1, "no new item after Wait");
}

#[test]
fn an_item_goes_when_the_session_stops_waiting() {
    let k = kit("wait-gone", seed_after_promote(&[(S1, 1)]));
    k.returns.add_waiting(S1, &node("m1")).unwrap();
    k.returns.tick();
    k.waiting.remove(S1).unwrap();
    k.returns.tick();
    assert_eq!(
        k.item(&format!("session-waits:{S1}")).unwrap().state,
        State::Withdrawn
    );
}

#[test]
fn continuing_records_a_claim_by_this_machine_only() {
    let k = kit("continue-claim", seed_after_promote(&[(S1, 1)]));
    k.returns.add_waiting(S1, &node("m1")).unwrap();
    k.returns.continue_here(S1).unwrap();
    let c = claim(S1, "m5", 2);
    let held = k.state().claims[S1].clone();
    assert_eq!((held.node, held.n), (c.node, c.n));
    assert_eq!(k.link.records.lock().unwrap()[0].1, OriginTier::Node);
}
