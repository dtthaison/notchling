use crate::cluster_fence_keeper_kit::{claim, mains, node, S1, S2};
use crate::cluster_move::Link;
use crate::cluster_returns_kit::{kit, seed_after_promote, Kit};
use notchling_engine::cluster::state::Command;
use notchling_engine::cluster::Tool;
use notchling_engine::inbox::{ActionId, Kind, State};

fn copies(k: &Kit) -> Vec<Command> {
    k.recorded()
        .into_iter()
        .filter(|c| matches!(c, Command::SideCopy { .. }))
        .collect()
}

#[test]
fn old_primary_raises_primary_back_with_take_back_and_keep() {
    let k = kit("back-item", seed_after_promote(&[(S1, 1)]));
    k.was_primary(1);
    k.returns.tick();
    let item = k.item("primary-back:2").expect("the item");
    assert_eq!(item.kind, Kind::PrimaryBack);
    assert_eq!(item.title, "m1 is primary");
    assert_eq!(item.state, State::Open);
    let actions: Vec<(ActionId, &str, bool)> = item
        .actions
        .iter()
        .map(|a| (a.id, a.label.as_str(), a.primary))
        .collect();
    assert_eq!(
        actions,
        vec![
            (ActionId::TakeBack, "Take Back", true),
            (ActionId::Keep, "Keep m1", false)
        ]
    );
}

#[test]
fn keep_closes_primary_back() {
    let k = kit("back-keep", seed_after_promote(&[(S1, 1)]));
    k.was_primary(1);
    k.returns.tick();
    let outcome =
        k.w.inbox
            .decide("primary-back:2", ActionId::Keep, "mac")
            .unwrap();
    assert!(outcome.answered);
    assert_eq!(k.item("primary-back:2").unwrap().state, State::Answered);
    k.returns.tick();
    assert!(k.link.app_calls.lock().unwrap().is_empty());
}

#[test]
fn a_machine_that_was_never_primary_raises_nothing() {
    let k = kit("back-never", seed_after_promote(&[(S1, 1)]));
    k.write_transcript(S1, 100);
    k.returns.tick();
    assert!(k.item("primary-back:2").is_none());
    assert!(k.recorded().is_empty());
}

#[test]
fn step_down_records_side_copies_for_grown_sessions() {
    let k = kit("step-grown", seed_after_promote(&[(S1, 1)]));
    let path = k.write_transcript(S1, 100);
    k.was_primary(1);
    k.returns.tick();
    let recorded = copies(&k);
    assert_eq!(recorded.len(), 1);
    let Command::SideCopy { copy } = &recorded[0] else {
        panic!("a side copy");
    };
    assert_eq!(copy.session, S1);
    assert_eq!(copy.node, node("m5"));
    assert_eq!(copy.base_set, "set");
    assert_eq!(copy.extra_bytes, 90);
    assert_eq!(copy.path.as_deref(), Some(path.to_str().unwrap()));
    assert!(path.exists());
    assert_eq!(k.state().side_copies.len(), 1);
}

#[test]
fn unclaimed_sessions_stay_and_get_no_side_copy() {
    let mut seed = seed_after_promote(&[(S1, 1)]);
    seed.main = mains(&[(S1, Tool::Claude), (S2, Tool::Claude)]);
    let k = kit("step-unclaimed", seed);
    k.write_transcript(S1, 100);
    k.write_transcript(S2, 500);
    k.was_primary(1);
    k.returns.tick();
    let sessions: Vec<String> = k
        .state()
        .side_copies
        .into_iter()
        .map(|c| c.session)
        .collect();
    assert_eq!(sessions, vec![S1.to_string()]);
}

#[test]
fn changed_folder_records_a_side_copy() {
    let k = kit("step-folder", seed_after_promote(&[(S1, 1)]));
    k.write_transcript(S1, 10);
    k.was_primary(1);
    *k.repo.0.lock().unwrap() = false;
    k.returns.tick();
    assert!(copies(&k).is_empty());

    let k = kit("step-folder-changed", seed_after_promote(&[(S1, 1)]));
    k.write_transcript(S1, 10);
    k.was_primary(1);
    *k.repo.0.lock().unwrap() = true;
    k.returns.tick();
    let recorded = copies(&k);
    assert_eq!(recorded.len(), 1);
    let Command::SideCopy { copy } = &recorded[0] else {
        panic!("a side copy");
    };
    assert!(copy.repo_changed);
    assert_eq!(copy.extra_bytes, 0);
}

#[test]
fn claims_arriving_after_step_down_still_record_side_copies() {
    let mut seed = seed_after_promote(&[]);
    seed.claims = Vec::new();
    let k = kit("step-late", seed);
    k.write_transcript(S1, 100);
    k.was_primary(1);
    k.returns.tick();
    assert!(k.item("primary-back:2").is_some());
    assert!(copies(&k).is_empty());

    k.link
        .record(
            Command::Claim {
                claim: claim(S1, "m1", 1),
            },
            notchling_engine::cluster::wire::OriginTier::Node,
            std::time::Duration::ZERO,
        )
        .unwrap();
    k.link.records.lock().unwrap().clear();
    k.returns.tick();
    assert_eq!(copies(&k).len(), 1);
    k.returns.tick();
    k.returns.tick();
    assert_eq!(copies(&k).len(), 1, "once per claim");
}

#[test]
fn a_failed_record_is_tried_again_on_the_next_tick() {
    let k = kit("step-retry", seed_after_promote(&[(S1, 1)]));
    k.write_transcript(S1, 100);
    k.was_primary(1);
    *k.link.fail.lock().unwrap() = Some("offline".to_string());
    k.returns.tick();
    assert!(k.state().side_copies.is_empty());
    assert!(k
        .item("side-copy-failed:aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa")
        .is_some());
    *k.link.fail.lock().unwrap() = None;
    k.returns.tick();
    assert_eq!(k.state().side_copies.len(), 1);
}

#[test]
fn taking_the_primary_back_withdraws_the_item() {
    let k = kit("step-home", seed_after_promote(&[(S1, 1)]));
    k.was_primary(1);
    k.returns.tick();
    assert_eq!(k.item("primary-back:2").unwrap().state, State::Open);
    let mut home = seed_after_promote(&[]);
    home.home = "m5";
    k.w.reseed(&home);
    k.returns.tick();
    assert_eq!(k.item("primary-back:2").unwrap().state, State::Withdrawn);
    let marker = std::fs::read_to_string(k.w.cluster.join("was-primary.json")).unwrap();
    assert!(marker.contains("\"term\":1"), "{marker}");
}

#[test]
fn a_restart_still_knows_it_was_primary() {
    let mut home = seed_after_promote(&[]);
    home.home = "m5";
    let k = kit("step-restart", home);
    k.returns.tick();
    assert!(k.w.cluster.join("was-primary.json").exists());
    k.w.reseed(&seed_after_promote(&[(S1, 1)]));
    let again = crate::cluster_returns::Returns::new(crate::cluster_returns::ReturnsParts {
        mover: k.mover.clone(),
        repo: k.repo.clone(),
        acting: k.acting.clone(),
        data: k.dir.clone(),
        waiting: k.waiting.clone(),
    });
    again.tick();
    assert!(k.item("primary-back:2").is_some());
}

#[test]
fn the_thread_ticks_and_closes() {
    let k = kit("step-thread", seed_after_promote(&[(S1, 1)]));
    k.was_primary(1);
    let thread = crate::cluster_returns_step::ReturnsThread::spawn(
        k.returns.clone(),
        std::time::Duration::from_millis(10),
    );
    crate::cluster_testkit::eventually("the item", || k.item("primary-back:2").is_some());
    thread.close();
    thread.close();
}

#[test]
fn unknown_repo_facts_count_as_changed() {
    use crate::cluster_returns::Repo;
    let repo = crate::cluster_returns_aside::GitRepo(std::sync::Arc::new(
        notchling_engine::control::launcher::FakeLauncher::new(),
    ));
    assert!(repo.changed("/work", None));
}

#[test]
fn a_session_still_running_here_is_checked_again_until_it_stops() {
    let k = kit("step-live", seed_after_promote(&[(S1, 1)]));
    k.write_transcript(S1, 10);
    k.rows
        .0
        .lock()
        .unwrap()
        .push(crate::cluster_returns_kit::agent_row(
            S1,
            "x",
            notchling_engine::sessions::AgentGroup::Working,
        ));
    k.was_primary(1);
    k.returns.tick();
    assert!(copies(&k).is_empty());
    k.write_transcript(S1, 60);
    k.rows.0.lock().unwrap().clear();
    k.returns.tick();
    let recorded = copies(&k);
    assert_eq!(recorded.len(), 1);
    let Command::SideCopy { copy } = &recorded[0] else {
        panic!("a side copy");
    };
    assert_eq!(copy.extra_bytes, 50);
    k.write_transcript(S1, 90);
    k.returns.tick();
    assert_eq!(copies(&k).len(), 1);
}

#[test]
fn a_primary_back_item_from_before_a_restart_is_withdrawn() {
    let k = kit("back-restart", seed_after_promote(&[(S1, 1)]));
    k.was_primary(1);
    k.returns.tick();
    assert_eq!(k.item("primary-back:2").unwrap().state, State::Open);
    let again = crate::cluster_returns::Returns::new(crate::cluster_returns::ReturnsParts {
        mover: k.mover.clone(),
        repo: k.repo.clone(),
        acting: k.acting.clone(),
        data: k.dir.clone(),
        waiting: k.waiting.clone(),
    });
    again.withdraw_back_except(None);
    assert_eq!(k.item("primary-back:2").unwrap().state, State::Withdrawn);
}
