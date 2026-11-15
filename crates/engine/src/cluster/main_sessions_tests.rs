use std::collections::{BTreeMap, BTreeSet};

use super::id::Tool;
use super::main_sessions::{is_main, may_be_main, reconcile, DAY_MS};
use super::state::{apply, start, Change, ClusterState, Command};
use super::testkit::member;
use crate::sessions::{Agent, AgentGroup, AgentTarget, NodeRef};

fn agent(id: &str) -> Agent {
    Agent {
        id: id.to_string(),
        name: id.to_string(),
        project: "repo".to_string(),
        cwd: "/work/repo".to_string(),
        group: AgentGroup::Completed,
        label: "Idle".to_string(),
        detail: None,
        since_ms: 5,
        target: AgentTarget::Job(id.to_string()),
        suggested_reply: None,
        transcript_path: Some(format!("/home/u/.claude/projects/p/{id}.jsonl")),
        host: "Background".to_string(),
        headless: false,
        config_dir: Some("/home/u/.claude".to_string()),
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
    }
}

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

#[test]
fn claude_and_codex_on_primary_are_main_by_default() {
    let kept = BTreeSet::new();
    for id in ["job-1", "codex:abc"] {
        let a = agent(id);
        assert_eq!(may_be_main(&a), Ok(()), "{id}");
        assert!(is_main(&a, &kept), "{id}");
    }
}

#[test]
fn kept_session_is_not_main() {
    let kept: BTreeSet<String> = ["job-1".to_string()].into();
    assert!(!is_main(&agent("job-1"), &kept));
    assert!(is_main(&agent("job-2"), &kept));
    assert_eq!(may_be_main(&agent("job-1")), Ok(()));
}

#[test]
fn agy_may_not() {
    let why = "Only Claude Code and Codex sessions on the primary can move".to_string();
    for id in ["agy:abc", "antigravity:abc"] {
        let a = agent(id);
        assert_eq!(may_be_main(&a), Err(why.clone()), "{id}");
        assert!(!is_main(&a, &BTreeSet::new()), "{id}");
    }
}

#[test]
fn worker_session_may_not() {
    let mut remote = agent("0123456789abcdef/job-1");
    remote.node = Some(NodeRef {
        id: "0123456789abcdef".to_string(),
        name: "m1".to_string(),
        online: true,
    });
    assert!(may_be_main(&remote).is_err());
    let mut shell = agent("shell:j1");
    shell.command = Some("make".to_string());
    assert!(may_be_main(&shell).is_err());
    let mut no_folder = agent("job-2");
    no_folder.cwd = String::new();
    assert!(may_be_main(&no_folder).is_err());
}

#[test]
fn set_main_is_journaled() {
    let mut state = created();
    let before = state.seq;
    let applied = apply(
        &mut state,
        Command::SetMain {
            session: "job-1".to_string(),
            main: false,
        },
        100,
    )
    .expect("set main");
    let entry = applied.entry.expect("journaled");
    assert_eq!(entry.seq, before + 1);
    assert_eq!(
        entry.command,
        Command::SetMain {
            session: "job-1".to_string(),
            main: false
        }
    );
    assert!(state.kept.contains("job-1"));
    assert!(applied
        .changes
        .iter()
        .any(|c| matches!(c, Change::Main { .. })));
    apply(
        &mut state,
        Command::SetMain {
            session: "job-1".to_string(),
            main: true,
        },
        110,
    )
    .expect("set main");
    assert!(state.kept.is_empty());
    assert_eq!(state.seq, before + 2);
}

#[test]
fn set_main_off_drops_a_main_session_and_the_monitor_does_not_bring_it_back() {
    let mut state = created();
    let seen = [agent("job-1")];
    let (main, ended) = reconcile(&state.main, &state.ended, &seen, &state.kept, 100);
    apply(&mut state, Command::Main { main, ended }, 100).expect("main");
    assert!(state.main.contains_key("job-1"));
    apply(
        &mut state,
        Command::SetMain {
            session: "job-1".to_string(),
            main: false,
        },
        110,
    )
    .expect("set main");
    assert!(!state.main.contains_key("job-1"));
    let (main, _) = reconcile(&state.main, &state.ended, &seen, &state.kept, 120);
    assert!(!main.contains_key("job-1"));
}

#[test]
fn ended_session_leaves_main_after_a_day() {
    let kept = BTreeSet::new();
    let live = [agent("job-1")];
    let (main, ended) = reconcile(&BTreeMap::new(), &BTreeMap::new(), &live, &kept, 1_000);
    assert_eq!(main["job-1"].tool, Tool::Claude);
    assert_eq!(main["job-1"].cwd, "/work/repo");
    assert_eq!(main["job-1"].since_ms, 1_000);
    let (main, ended) = reconcile(&main, &ended, &[], &kept, 2_000);
    assert!(main.contains_key("job-1"));
    let (main, ended) = reconcile(&main, &ended, &[], &kept, 2_000 + DAY_MS - 1);
    assert!(main.contains_key("job-1"));
    let back = reconcile(&main, &ended, &live, &kept, 3_000 + DAY_MS);
    assert!(back.0.contains_key("job-1"));
    assert!(back.1.is_empty());
    let (main, ended) = reconcile(&main, &ended, &[], &kept, 2_000 + DAY_MS);
    assert!(main.is_empty());
    assert!(ended.is_empty());
}

#[test]
fn phone_started_session_is_main() {
    let kept = BTreeSet::new();
    let mut started = agent("codex:phone-1");
    started.group = AgentGroup::Working;
    started.since_ms = 50;
    started.config_dir = None;
    let (main, _) = reconcile(&BTreeMap::new(), &BTreeMap::new(), &[started], &kept, 60);
    let one = &main["codex:phone-1"];
    assert_eq!(one.tool, Tool::Codex);
    assert_eq!(one.config_dir, None);
    assert_eq!(one.since_ms, 60);
}
