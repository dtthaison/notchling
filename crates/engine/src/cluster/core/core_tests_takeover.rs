use std::collections::BTreeMap;

use serde_json::json;

use super::core_tests::{answers, Hub};
use super::{Effect, LocalCmd, Takeover};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::{job_id, Job, JobSpec, JobState};
use crate::cluster::main_sessions::MainSession;
use crate::cluster::members::Term;
use crate::cluster::state::Change;
use crate::cluster::testkit::id;

fn trio() -> (Hub, NodeId, NodeId) {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.join("m5", "w1", "linux", false);
    hub.link(&m1, &id("w1"));
    hub.tick_all(6_000);
    hub.tick_all(6_000);
    (hub, m5, m1)
}

fn takeover_for(hub: &Hub, from: &NodeId, to: &NodeId) -> Takeover {
    let state = hub.core(from).state().expect("state");
    Takeover {
        id: state.id.clone(),
        name: state.name.clone(),
        term: Term {
            n: state.term.n + 1,
            node: to.clone(),
        },
        vault: state.vault.clone(),
        members: hub.core(from).shared_members(),
        sessions: Vec::new(),
        restored: None,
    }
}

fn take(hub: &mut Hub, who: &NodeId, takeover: Takeover) -> Vec<Effect> {
    hub.local(
        who,
        LocalCmd::TakePrimary {
            takeover: Box::new(takeover),
        },
    )
}

#[test]
fn a_standby_takes_the_next_term_and_the_old_primary_steps_down() {
    let (mut hub, m5, m1) = trio();
    let before = hub.core(&m5).state().expect("state").term.n;
    let takeover = takeover_for(&hub, &m5, &m1);
    let effects = take(&mut hub, &m1, takeover);
    assert!(answers(&effects)[0].is_ok());
    let new = hub.core(&m1).state().expect("state");
    assert_eq!(new.home, m1);
    assert_eq!(
        new.term,
        Term {
            n: before + 1,
            node: m1.clone()
        }
    );
    let old = hub.core(&m5).state().expect("state");
    assert_eq!(old.home, m1);
    assert_eq!(old.term, new.term);
    assert!(hub.core(&m1).is_home());
    assert!(!hub.core(&m5).is_home());
    let w1 = id("w1");
    assert_eq!(hub.core(&w1).state().expect("state").home, m1);
    assert!(hub
        .seen
        .iter()
        .any(|(name, e)| name == "m5" && matches!(e, Effect::SteppedDown { .. })));
}

#[test]
fn a_repeated_takeover_changes_nothing() {
    let (mut hub, m5, m1) = trio();
    let takeover = takeover_for(&hub, &m5, &m1);
    take(&mut hub, &m1, takeover.clone());
    let seq = hub.core(&m1).state().expect("state").seq;
    let effects = take(&mut hub, &m1, takeover);
    assert!(answers(&effects)[0].is_ok());
    assert_eq!(hub.core(&m1).state().expect("state").seq, seq);
    assert!(!effects.iter().any(|e| matches!(e, Effect::Persist { .. })));
}

#[test]
fn a_takeover_that_does_not_fit_is_refused() {
    let (mut hub, m5, m1) = trio();
    let w1 = id("w1");

    let mut another = takeover_for(&hub, &m5, &m1);
    another.term.node = m5.clone();
    let effects = take(&mut hub, &m1, another);
    assert!(matches!(answers(&effects)[0], Err(e) if e.code == "refused"));

    let mut stale = takeover_for(&hub, &m5, &m1);
    stale.term.n = 0;
    let effects = take(&mut hub, &m1, stale);
    assert!(matches!(answers(&effects)[0], Err(e) if e.code == "refused"));

    let mut foreign = takeover_for(&hub, &m5, &m1);
    foreign.id = "other".to_string();
    let effects = take(&mut hub, &m1, foreign);
    assert!(matches!(answers(&effects)[0], Err(e) if e.code == "refused"));

    let worker = takeover_for(&hub, &m5, &w1);
    let effects = take(&mut hub, &w1, worker);
    assert!(matches!(answers(&effects)[0], Err(e) if e.code == "refused"));

    assert_eq!(hub.core(&m1).state().expect("state").home, m5);
    assert_eq!(hub.core(&m5).state().expect("state").home, m5);
}

fn restored_state(hub: &Hub, from: &NodeId, seq: u64) -> crate::cluster::state::ClusterState {
    let mut state = hub.core(from).state().expect("state").clone();
    state.seq = seq;
    let spec = JobSpec {
        node: id("w1"),
        tool: Tool::Shell,
        cwd: "/srv".to_string(),
        args: json!({}),
        secrets: Vec::new(),
    };
    let job = Job {
        id: job_id("k1"),
        key: "k1".to_string(),
        spec,
        state: JobState::Dispatching,
        term: state.term.clone(),
        created_ms: 5,
        session: None,
        detail: None,
    };
    state.jobs = BTreeMap::from([(job.id.clone(), job)]);
    state.main = BTreeMap::from([(
        "sess-1".to_string(),
        MainSession {
            id: "sess-1".to_string(),
            tool: Tool::Claude,
            cwd: "/work".to_string(),
            config_dir: None,
            transcript: None,
            respawn: vec!["--name".to_string(), "x".to_string()],
            since_ms: 1,
        },
    )]);
    state
}

#[test]
fn a_takeover_keeps_the_restored_jobs_and_main_and_persists_them_after_the_old_journal() {
    let (mut hub, m5, m1) = trio();
    let restored = restored_state(&hub, &m5, 900);
    let mut takeover = takeover_for(&hub, &m5, &m1);
    takeover.restored = Some(restored);
    let effects = take(&mut hub, &m1, takeover);
    assert!(answers(&effects)[0].is_ok());
    let state = hub.core(&m1).state().expect("state");
    assert_eq!(state.jobs.len(), 1);
    assert_eq!(state.main["sess-1"].respawn.len(), 2);
    assert!(state.seq > 900);
    let persisted: Vec<&Effect> = effects
        .iter()
        .filter(|e| matches!(e, Effect::Persist { .. }))
        .collect();
    assert_eq!(persisted.len(), 1);
    let Effect::Persist { entry, changes } = persisted[0] else {
        unreachable!()
    };
    assert!(entry.seq > 900);
    assert!(changes.iter().any(|c| matches!(c, Change::Job(_))));
    assert!(changes.iter().any(|c| matches!(c, Change::Main { .. })));
    assert!(changes.iter().any(|c| matches!(c, Change::Cluster { .. })));
    let members = changes
        .iter()
        .filter(|c| matches!(c, Change::Member(_)))
        .count();
    assert_eq!(members, 3);
}

#[test]
fn a_member_missing_from_the_restored_database_is_dropped_there() {
    let (mut hub, m5, m1) = trio();
    let mut restored = restored_state(&hub, &m5, 10);
    let ghost = id("ghost");
    let mut stale = crate::cluster::testkit::member("ghost", "linux", false);
    stale.id = ghost.clone();
    restored.members.insert(ghost.clone(), stale);
    let mut takeover = takeover_for(&hub, &m5, &m1);
    takeover.restored = Some(restored);
    let effects = take(&mut hub, &m1, takeover);
    let Some(Effect::Persist { changes, .. }) =
        effects.iter().find(|e| matches!(e, Effect::Persist { .. }))
    else {
        panic!("nothing was persisted");
    };
    assert!(changes
        .iter()
        .any(|c| matches!(c, Change::Dropped(id) if *id == ghost)));
    assert!(!hub
        .core(&m1)
        .state()
        .expect("state")
        .members
        .contains_key(&ghost));
}
