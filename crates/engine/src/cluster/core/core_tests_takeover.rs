use std::collections::BTreeMap;

use serde_json::json;

use super::core_tests::{answers, dispatch_cmd, Hub};
use super::{call_wait, Effect, LocalCmd, Takeover, CALL_MS, MOVE_CALL_MS};
use crate::cluster::claims::{Claim, SideCopy};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::{job_id, Job, JobSpec, JobState};
use crate::cluster::main_sessions::MainSession;
use crate::cluster::members::Term;
use crate::cluster::persist::{load, save};
use crate::cluster::state::Change;
use crate::cluster::testkit::id;
use crate::cluster::wire::Msg;
use crate::sessions::testutil::TempDir;

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
        set: None,
        claims: Vec::new(),
        restored: None,
        staged: false,
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

#[test]
fn l5_a_takeover_refused_late_changes_nothing() {
    let (mut hub, m5, m1) = trio();
    let before = hub.core(&m1).state().expect("state").clone();
    let mut restored = restored_state(&hub, &m5, 900);
    restored.id = "another".to_string();
    let mut takeover = takeover_for(&hub, &m5, &m1);
    takeover.restored = Some(restored);
    takeover.staged = true;
    let effects = take(&mut hub, &m1, takeover);
    assert!(matches!(answers(&effects)[0], Err(e) if e.code == "refused"));
    assert_eq!(hub.core(&m1).state().expect("state"), &before);
    assert!(!effects
        .iter()
        .any(|e| matches!(e, Effect::AdoptStaged | Effect::Persist { .. })));

    let mut ineligible = takeover_for(&hub, &m5, &m1);
    ineligible.restored = Some(restored_state(&hub, &m5, 900));
    ineligible.staged = true;
    for m in &mut ineligible.members {
        if m.id == m1 {
            m.can_be_master = false;
        }
    }
    let effects = take(&mut hub, &m1, ineligible);
    assert!(matches!(answers(&effects)[0], Err(e) if e.code == "refused"));
    assert_eq!(hub.core(&m1).state().expect("state"), &before);
    assert!(!effects.iter().any(|e| matches!(e, Effect::AdoptStaged)));
}

#[test]
fn m5_a_staged_takeover_adopts_the_database_right_before_it_persists() {
    let (mut hub, m5, m1) = trio();
    let mut takeover = takeover_for(&hub, &m5, &m1);
    takeover.restored = Some(restored_state(&hub, &m5, 900));
    takeover.staged = true;
    let effects = take(&mut hub, &m1, takeover.clone());
    assert!(answers(&effects)[0].is_ok());
    let adopt = effects
        .iter()
        .position(|e| matches!(e, Effect::AdoptStaged))
        .expect("the staged database is adopted");
    let persist = effects
        .iter()
        .position(|e| matches!(e, Effect::Persist { .. }))
        .expect("persisted");
    assert_eq!(adopt + 1, persist);
    let again = take(&mut hub, &m1, takeover);
    assert!(answers(&again)[0].is_ok());
    assert!(!again.iter().any(|e| matches!(e, Effect::AdoptStaged)));
}

#[test]
fn m7_a_fenced_primary_refuses_dispatch_and_main_changes() {
    let (mut hub, m5, _) = trio();
    let why = "The primary is moving to m1".to_string();
    hub.local(
        &m5,
        LocalCmd::Fence {
            reason: Some(why.clone()),
        },
    );
    let effects = hub.local(&m5, dispatch_cmd("k", "w1", Tool::Shell));
    assert!(matches!(answers(&effects)[0], Err(e) if e.message == why && e.code == "refused"));
    let effects = hub.local(
        &m5,
        LocalCmd::SetMain {
            session: "sess-9".to_string(),
            main: false,
        },
    );
    assert!(matches!(answers(&effects)[0], Err(e) if e.message == why));
    let state = hub.core(&m5).state().expect("state").clone();
    hub.local(
        &m5,
        LocalCmd::MainSeen {
            agents: Vec::new(),
            codex_exec: Default::default(),
            respawn: Default::default(),
        },
    );
    assert_eq!(hub.core(&m5).state().expect("state"), &state);
    assert!(state.jobs.is_empty());

    hub.local(&m5, LocalCmd::Fence { reason: None });
    let effects = hub.local(
        &m5,
        LocalCmd::SetMain {
            session: "sess-9".to_string(),
            main: false,
        },
    );
    assert!(answers(&effects)[0].is_ok());
}

#[test]
fn m2_move_calls_wait_for_what_the_target_does() {
    let three = json!({"sessions": ["a", "b", "c"]});
    assert_eq!(
        call_wait("move.resume", &three),
        crate::cluster::move_home::resume_call_ms(3)
    );
    assert!(call_wait("move.resume", &three) > CALL_MS);
    assert_eq!(call_wait("move.switch", &json!({})), MOVE_CALL_MS);
    assert_eq!(call_wait("session.reply", &json!({})), CALL_MS);
}

fn claim(session: &str, node: &NodeId, n: u64, at: &NodeId) -> Claim {
    Claim {
        session: session.to_string(),
        node: node.clone(),
        n,
        term: Term {
            n: 1,
            node: at.clone(),
        },
        base_set: "set-1".to_string(),
        base_len: 10,
        touched: false,
    }
}

fn claims_of(hub: &Hub, who: &NodeId) -> Vec<Claim> {
    hub.core(who)
        .state()
        .expect("state")
        .claims
        .values()
        .cloned()
        .collect()
}

#[test]
fn r175_the_handover_carries_the_old_primarys_claims() {
    let (mut hub, m5, m1) = trio();
    let late = claim("sess-1", &m5, 7, &m5);
    hub.core_mut(&m5)
        .state
        .as_mut()
        .expect("state")
        .claims
        .insert("sess-1".to_string(), late.clone());
    let effects = hub.local(
        &m5,
        LocalCmd::MoveSwitch {
            to: m1.clone(),
            sessions: vec!["sess-1".to_string()],
            set: None,
        },
    );
    let sent: Vec<Takeover> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::Send {
                msg: Msg::Call { op, args, .. },
                ..
            } if op == "move.switch" => serde_json::from_value((**args).clone()).ok(),
            _ => None,
        })
        .collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].claims, vec![late]);
}

#[test]
fn r175_a_primary_restored_from_a_set_gets_the_claims_made_after_it() {
    let (mut hub, m5, m1) = trio();
    let late = claim("sess-1", &m5, 7, &m5);
    let mut restored = restored_state(&hub, &m5, 900);
    restored.claims.clear();
    let mut takeover = takeover_for(&hub, &m5, &m1);
    takeover.restored = Some(restored);
    takeover.claims = vec![late.clone()];
    let effects = take(&mut hub, &m1, takeover);
    assert!(answers(&effects)[0].is_ok());
    assert_eq!(claims_of(&hub, &m1), vec![late.clone()]);
    let persisted = effects.iter().find_map(|e| match e {
        Effect::Persist { changes, .. } => changes.iter().find_map(|c| match c {
            Change::Claims(claims) => Some(claims.clone()),
            _ => None,
        }),
        _ => None,
    });
    assert_eq!(persisted.expect("claims persisted")["sess-1"], late);
}

#[test]
fn r175_the_stronger_claim_wins_whichever_side_it_comes_from() {
    let (mut hub, m5, m1) = trio();
    let old = claim("sess-1", &m5, 3, &m5);
    let newer = claim("sess-1", &m1, 8, &m5);
    let other = claim("sess-2", &m1, 1, &m5);
    let mut restored = restored_state(&hub, &m5, 900);
    restored.claims = BTreeMap::from([
        ("sess-1".to_string(), old),
        ("sess-2".to_string(), other.clone()),
    ]);
    let mut takeover = takeover_for(&hub, &m5, &m1);
    takeover.restored = Some(restored);
    takeover.claims = vec![newer.clone()];
    let effects = take(&mut hub, &m1, takeover);
    assert!(answers(&effects)[0].is_ok());
    assert_eq!(claims_of(&hub, &m1), vec![newer, other]);
}

#[test]
fn r175_a_takeover_from_an_older_node_without_claims_changes_none() {
    let (mut hub, m5, m1) = trio();
    let mut value = serde_json::to_value(takeover_for(&hub, &m5, &m1)).unwrap();
    value.as_object_mut().unwrap().remove("claims");
    let takeover: Takeover = serde_json::from_value(value).unwrap();
    assert!(takeover.claims.is_empty());
    let own = claim("sess-1", &m1, 2, &m5);
    hub.core_mut(&m1)
        .state
        .as_mut()
        .expect("state")
        .claims
        .insert("sess-1".to_string(), own.clone());
    let effects = take(&mut hub, &m1, takeover);
    assert!(answers(&effects)[0].is_ok());
    assert_eq!(claims_of(&hub, &m1), vec![own]);
}

fn copy(session: &str, node: &NodeId, base: &str, extra: u64) -> SideCopy {
    SideCopy {
        session: session.to_string(),
        node: node.clone(),
        base_set: base.to_string(),
        extra_bytes: extra,
        repo_changed: false,
        path: None,
        at_ms: 7,
        hidden: false,
    }
}

#[test]
fn a_restored_takeover_keeps_the_side_copies_recorded_after_the_snapshot() {
    let (mut hub, m5, m1) = trio();
    let mut live_hidden = copy("sess-1", &m1, "set-1", 90);
    live_hidden.hidden = true;
    live_hidden.path = Some("/moved/here".to_string());
    let live_new = copy("sess-2", &m1, "set-2", 5);
    hub.core_mut(&m1).state.as_mut().expect("state").side_copies =
        vec![live_hidden.clone(), live_new.clone()];
    let mut restored = restored_state(&hub, &m5, 900);
    restored.side_copies = vec![copy("sess-1", &m1, "set-1", 40)];
    let mut takeover = takeover_for(&hub, &m5, &m1);
    takeover.restored = Some(restored);
    let effects = take(&mut hub, &m1, takeover);
    assert!(answers(&effects)[0].is_ok());
    let want = vec![live_hidden, live_new];
    assert_eq!(hub.core(&m1).state().expect("state").side_copies, want);
    let tmp = TempDir::new("takeover-copies");
    let dir = tmp.path("cluster");
    for e in &effects {
        if let Effect::Persist { entry, changes } = e {
            save(&dir, entry, changes).expect("save");
        }
    }
    let loaded = load(&dir).expect("load").expect("state");
    assert_eq!(loaded.side_copies, want);
}

#[test]
fn a_restored_takeover_adds_the_snapshots_own_side_copies() {
    let (mut hub, m5, m1) = trio();
    let mut restored = restored_state(&hub, &m5, 900);
    let old = copy("sess-1", &m5, "set-1", 40);
    restored.side_copies = vec![old.clone()];
    let mut takeover = takeover_for(&hub, &m5, &m1);
    takeover.restored = Some(restored);
    let effects = take(&mut hub, &m1, takeover);
    assert!(answers(&effects)[0].is_ok());
    assert_eq!(hub.core(&m1).state().expect("state").side_copies, vec![old]);
}
