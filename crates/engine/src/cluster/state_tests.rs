use serde_json::json;

use super::id::{Role, Tool};
use super::jobs::{job_id, JobSpec, JobState};
use super::members::{can_be_master_default, Desired, MemberState, Term};
use super::state::{apply, role_of, start, ClusterState, Command, CommandError};
use super::testkit::{id, member, versions};

fn created() -> ClusterState {
    let (state, _, _) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home: member("m5", "macos", true),
        },
        10,
    )
    .expect("create");
    state
}

fn joined(state: &mut ClusterState, name: &str, os: &str, can_be_master: bool) {
    let m = member(name, os, false);
    apply(
        state,
        Command::ProposeJoin {
            member: m.clone(),
            item: format!("node-join:{}", m.id),
        },
        20,
    )
    .expect("propose");
    apply(
        state,
        Command::Approve {
            node: m.id,
            client: false,
            can_be_master,
        },
        30,
    )
    .expect("approve");
}

fn spec(node: &str) -> JobSpec {
    JobSpec {
        node: id(node),
        tool: Tool::Codex,
        cwd: "/srv/repo".to_string(),
        args: json!({"prompt": "go"}),
        secrets: Vec::new(),
    }
}

#[test]
fn create_sets_home_and_term_one() {
    let state = created();
    assert_eq!(state.home, id("m5"));
    assert_eq!(
        state.term,
        Term {
            n: 1,
            node: id("m5")
        }
    );
    let home = &state.members[&id("m5")];
    assert_eq!(home.state, MemberState::Active);
    assert!(home.can_be_master);
    assert_eq!(state.seq, 1);
}

#[test]
fn create_sets_vault_to_home() {
    let state = created();
    assert_eq!(state.vault, id("m5"));
}

#[test]
fn create_twice_is_refused() {
    let mut state = created();
    let again = apply(
        &mut state,
        Command::Create {
            id: "c2".to_string(),
            name: "x".to_string(),
            home: member("m1", "macos", true),
        },
        11,
    );
    assert_eq!(again.err(), Some(CommandError::Exists));
}

#[test]
fn terms_order_by_n_then_node() {
    let a = Term {
        n: 1,
        node: id("w1"),
    };
    let b = Term {
        n: 2,
        node: id("m5"),
    };
    assert!(a < b);
    let (c, d) = (
        Term {
            n: 2,
            node: id("m1"),
        },
        Term {
            n: 2,
            node: id("m5"),
        },
    );
    assert_eq!(c < d, id("m1") < id("m5"));
}

#[test]
fn join_is_pending_until_approved() {
    let mut state = created();
    let m = member("w1", "linux", false);
    let applied = apply(
        &mut state,
        Command::ProposeJoin {
            member: m.clone(),
            item: "node-join:w1".to_string(),
        },
        20,
    )
    .expect("propose");
    assert!(applied.entry.is_some());
    assert_eq!(state.members[&m.id].state, MemberState::Pending);
    assert!(!state.members[&m.id].can_be_master);
    let early = apply(
        &mut state,
        Command::Dispatch {
            key: "k".to_string(),
            spec: spec("w1"),
        },
        21,
    );
    assert_eq!(early.err(), Some(CommandError::NotActive(m.id.clone())));
    apply(
        &mut state,
        Command::Approve {
            node: m.id.clone(),
            client: false,
            can_be_master: false,
        },
        22,
    )
    .expect("approve");
    assert_eq!(state.members[&m.id].state, MemberState::Active);
}

#[test]
fn deny_drops_the_pending_member() {
    let mut state = created();
    let m = member("w1", "linux", false);
    apply(
        &mut state,
        Command::ProposeJoin {
            member: m.clone(),
            item: "i".to_string(),
        },
        20,
    )
    .expect("propose");
    apply(&mut state, Command::Deny { node: m.id.clone() }, 21).expect("deny");
    assert!(!state.members.contains_key(&m.id));
    assert_eq!(
        apply(&mut state, Command::Deny { node: m.id.clone() }, 22).err(),
        Some(CommandError::Unknown(m.id))
    );
}

#[test]
fn home_cannot_be_removed() {
    let mut state = created();
    let err = apply(&mut state, Command::Remove { node: id("m5") }, 20);
    assert_eq!(err.err(), Some(CommandError::BadTransition));
}

#[test]
fn role_follows_home_and_can_be_master() {
    let mut state = created();
    joined(&mut state, "m1", "macos", true);
    joined(&mut state, "m2", "macos", true);
    joined(&mut state, "w1", "linux", false);
    assert_eq!(role_of(&state, &id("m5")), Role::Home);
    assert_eq!(role_of(&state, &id("m1")), Role::Standby);
    assert_eq!(role_of(&state, &id("m2")), Role::Standby);
    assert_eq!(role_of(&state, &id("w1")), Role::Worker);
    apply(
        &mut state,
        Command::SetMember {
            node: id("m5"),
            can_be_master: Some(false),
            client: None,
            reference: None,
            accepts_delegation: None,
            desired: None,
        },
        40,
    )
    .expect("set");
    assert_eq!(role_of(&state, &id("m5")), Role::Home);
    assert_eq!(role_of(&state, &id("nobody")), Role::Worker);
}

#[test]
fn can_be_master_defaults_on_for_macs_only() {
    assert!(can_be_master_default("macos"));
    assert!(!can_be_master_default("linux"));
    assert!(!can_be_master_default("windows"));
}

#[test]
fn removed_member_is_refused_and_never_dispatched() {
    let mut state = created();
    joined(&mut state, "w1", "linux", false);
    apply(&mut state, Command::Remove { node: id("w1") }, 40).expect("remove");
    assert_eq!(state.members[&id("w1")].state, MemberState::Removed);
    let err = apply(
        &mut state,
        Command::Dispatch {
            key: "k".to_string(),
            spec: spec("w1"),
        },
        41,
    );
    assert_eq!(err.err(), Some(CommandError::Removed(id("w1"))));
    let again = apply(&mut state, Command::Remove { node: id("w1") }, 42);
    assert_eq!(again.err(), Some(CommandError::Removed(id("w1"))));
}

#[test]
fn a_removed_node_may_ask_to_join_again() {
    let mut state = created();
    joined(&mut state, "w1", "linux", false);
    apply(&mut state, Command::Remove { node: id("w1") }, 40).expect("remove");
    apply(
        &mut state,
        Command::ProposeJoin {
            member: member("w1", "linux", false),
            item: "i".to_string(),
        },
        41,
    )
    .expect("propose");
    assert_eq!(state.members[&id("w1")].state, MemberState::Pending);
}

#[test]
fn same_key_dispatch_is_one_job() {
    let mut state = created();
    joined(&mut state, "w1", "linux", false);
    let first = apply(
        &mut state,
        Command::Dispatch {
            key: "k".to_string(),
            spec: spec("w1"),
        },
        50,
    )
    .expect("dispatch");
    assert!(first.entry.is_some());
    let seq = state.seq;
    let again = apply(
        &mut state,
        Command::Dispatch {
            key: "k".to_string(),
            spec: spec("w1"),
        },
        51,
    )
    .expect("dispatch");
    assert!(again.entry.is_none());
    assert!(again.changes.is_empty());
    assert_eq!(state.seq, seq);
    assert_eq!(state.jobs.len(), 1);
}

#[test]
fn job_ids_come_from_keys() {
    let id1 = job_id("k1");
    assert!(id1.starts_with("j-"));
    assert_eq!(id1.len(), 14);
    assert_eq!(id1, job_id("k1"));
    assert_ne!(id1, job_id("k2"));
}

#[test]
fn job_state_moves_forward_only() {
    let mut state = created();
    joined(&mut state, "w1", "linux", false);
    apply(
        &mut state,
        Command::Dispatch {
            key: "k".to_string(),
            spec: spec("w1"),
        },
        50,
    )
    .expect("dispatch");
    let job = job_id("k");
    let go = |state: &mut ClusterState, next| {
        apply(
            state,
            Command::JobState {
                id: job.clone(),
                state: next,
                session: Some("s1".to_string()),
                detail: None,
            },
            60,
        )
    };
    assert_eq!(
        go(&mut state, JobState::Done).err(),
        Some(CommandError::BadTransition)
    );
    go(&mut state, JobState::Running).expect("running");
    assert_eq!(state.jobs[&job].session.as_deref(), Some("s1"));
    assert_eq!(
        go(&mut state, JobState::Dispatching).err(),
        Some(CommandError::BadTransition)
    );
    go(&mut state, JobState::Done).expect("done");
    assert_eq!(
        go(&mut state, JobState::Running).err(),
        Some(CommandError::BadTransition)
    );
    assert_eq!(
        go(&mut state, JobState::Failed).err(),
        Some(CommandError::BadTransition)
    );
}

#[test]
fn seq_grows_by_one_per_entry() {
    let mut state = created();
    let mut last = state.seq;
    let m = member("w1", "linux", false);
    for command in [
        Command::ProposeJoin {
            member: m.clone(),
            item: "i".to_string(),
        },
        Command::Approve {
            node: m.id.clone(),
            client: false,
            can_be_master: false,
        },
        Command::Versions {
            node: m.id.clone(),
            versions: versions("linux"),
        },
    ] {
        let entry = apply(&mut state, command, 70)
            .expect("apply")
            .entry
            .expect("entry");
        assert_eq!(entry.seq, last + 1);
        last = entry.seq;
    }
    assert_eq!(state.seq, last);
}

#[test]
fn a_newer_term_moves_the_home() {
    let mut state = created();
    joined(&mut state, "m1", "macos", true);
    let next = Term {
        n: 2,
        node: id("m1"),
    };
    apply(&mut state, Command::Term { term: next.clone() }, 80).expect("term");
    assert_eq!(state.home, id("m1"));
    assert_eq!(state.term, next);
    let older = apply(
        &mut state,
        Command::Term {
            term: Term {
                n: 1,
                node: id("m5"),
            },
        },
        81,
    );
    assert_eq!(older.err(), Some(CommandError::BadTransition));
}

#[test]
fn sync_replaces_members_and_drops_the_missing() {
    let mut state = created();
    joined(&mut state, "w1", "linux", false);
    let mut other = member("w2", "linux", false);
    other.state = MemberState::Active;
    other.desired = Desired {
        acting: true,
        bridge: false,
        install: false,
    };
    let home = state.members[&id("m5")].clone();
    let term = state.term.clone();
    let applied = apply(
        &mut state,
        Command::Sync {
            id: "c1".to_string(),
            name: "home".to_string(),
            term: term.clone(),
            home: id("m5"),
            vault: id("m5"),
            members: vec![home, other.clone()],
        },
        90,
    )
    .expect("sync");
    assert!(state.members.contains_key(&other.id));
    assert!(!state.members.contains_key(&id("w1")));
    assert!(applied
        .changes
        .iter()
        .any(|c| matches!(c, super::state::Change::Dropped(n) if *n == id("w1"))));
    let term = state.term.clone();
    let wrong = apply(
        &mut state,
        Command::Sync {
            id: "other".to_string(),
            name: "x".to_string(),
            term: term.clone(),
            home: id("m5"),
            vault: id("m5"),
            members: Vec::new(),
        },
        91,
    );
    assert_eq!(wrong.err(), Some(CommandError::BadTransition));
}

#[test]
fn state_round_trips_through_json() {
    let mut state = created();
    joined(&mut state, "w1", "linux", false);
    apply(
        &mut state,
        Command::Dispatch {
            key: "k".to_string(),
            spec: spec("w1"),
        },
        50,
    )
    .expect("dispatch");
    let text = serde_json::to_string(&state).expect("json");
    let back: ClusterState = serde_json::from_str(&text).expect("state");
    assert_eq!(back, state);
}
