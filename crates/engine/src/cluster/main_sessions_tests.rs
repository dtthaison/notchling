use std::collections::{BTreeMap, BTreeSet};

use super::id::Tool;
use super::main_sessions::{
    is_main, may_be_main, reconcile, valid_id, DAY_MS, EXITED, HEADLESS, NOT_YET,
};
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
        target: AgentTarget::Job("ab12cd34".to_string()),
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
    let (main, ended, kept) = reconcile(
        &state.main,
        &state.ended,
        &seen,
        (&state.kept, &BTreeSet::new()),
        100,
    );
    apply(&mut state, Command::Main { main, ended, kept }, 100).expect("main");
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
    let (main, _, _) = reconcile(
        &state.main,
        &state.ended,
        &seen,
        (&state.kept, &BTreeSet::new()),
        120,
    );
    assert!(!main.contains_key("job-1"));
}

#[test]
fn ended_session_leaves_main_after_a_day() {
    let kept = (&BTreeSet::new(), &BTreeSet::new());
    let live = [agent("job-1")];
    let (main, ended, _) = reconcile(&BTreeMap::new(), &BTreeMap::new(), &live, kept, 1_000);
    assert_eq!(main["job-1"].tool, Tool::Claude);
    assert_eq!(main["job-1"].cwd, "/work/repo");
    assert_eq!(main["job-1"].since_ms, 1_000);
    let (main, ended, _) = reconcile(&main, &ended, &[], kept, 2_000);
    assert!(main.contains_key("job-1"));
    let (main, ended, _) = reconcile(&main, &ended, &[], kept, 2_000 + DAY_MS - 1);
    assert!(main.contains_key("job-1"));
    let back = reconcile(&main, &ended, &live, kept, 3_000 + DAY_MS);
    assert!(back.0.contains_key("job-1"));
    assert!(back.1.is_empty());
    let (main, ended, _) = reconcile(&main, &ended, &[], kept, 2_000 + DAY_MS);
    assert!(main.is_empty());
    assert!(ended.is_empty());
}

#[test]
fn phone_started_session_is_main() {
    let mut job = agent("1f2e3d4c-aaaa-bbbb-cccc-000000000001");
    job.group = AgentGroup::Working;
    job.since_ms = 50;
    job.host = "Background".to_string();
    job.config_dir = None;
    let mut codex = agent("codex:phone-1");
    codex.target = AgentTarget::Resume {
        session_id: "phone-1".to_string(),
        cwd: "/work/repo".to_string(),
    };
    let exec: BTreeSet<String> = ["codex:phone-1".to_string()].into();
    let (main, _, _) = reconcile(
        &BTreeMap::new(),
        &BTreeMap::new(),
        &[job, codex],
        (&BTreeSet::new(), &exec),
        60,
    );
    let one = &main["1f2e3d4c-aaaa-bbbb-cccc-000000000001"];
    assert_eq!(one.tool, Tool::Claude);
    assert_eq!(one.config_dir, None);
    assert_eq!(one.since_ms, 60);
    assert_eq!(main["codex:phone-1"].tool, Tool::Codex);
}

#[test]
fn headless_session_may_not() {
    let mut headless = agent("sess-1");
    headless.headless = true;
    assert_eq!(may_be_main(&headless), Err(HEADLESS.to_string()));
    assert!(!is_main(&headless, &BTreeSet::new()));
}

#[test]
fn a_job_is_main_only_once_it_has_its_session_id_and_a_transcript() {
    let mut fresh = agent("ab12cd34");
    assert_eq!(may_be_main(&fresh), Err(NOT_YET.to_string()));
    fresh.id = "sess-1".to_string();
    fresh.transcript_path = None;
    assert_eq!(may_be_main(&fresh), Err(NOT_YET.to_string()));
    fresh.transcript_path = Some("/t.jsonl".to_string());
    assert_eq!(may_be_main(&fresh), Ok(()));
    let (main, _, _) = reconcile(
        &BTreeMap::new(),
        &BTreeMap::new(),
        &[agent("ab12cd34")],
        (&BTreeSet::new(), &BTreeSet::new()),
        10,
    );
    assert!(main.is_empty());
}

#[test]
fn a_terminal_session_has_its_session_id_and_needs_no_transcript() {
    let mut terminal = agent("sess-t");
    terminal.target = AgentTarget::Process(4242);
    terminal.transcript_path = None;
    assert_eq!(may_be_main(&terminal), Ok(()));
}

#[test]
fn keep_chosen_on_the_short_id_holds_after_the_session_id_appears() {
    let kept: BTreeSet<String> = ["ab12cd34".to_string()].into();
    assert!(!is_main(&agent("sess-1"), &kept));
    let by_session: BTreeSet<String> = ["sess-1".to_string()].into();
    assert!(!is_main(&agent("sess-1"), &by_session));
    assert!(is_main(&agent("sess-2"), &BTreeSet::new()));
}

#[test]
fn codex_is_main_only_when_notchling_started_it() {
    let mut codex = agent("codex:c1");
    codex.target = AgentTarget::Resume {
        session_id: "c1".to_string(),
        cwd: "/work/repo".to_string(),
    };
    let none = BTreeSet::new();
    let (main, _, _) = reconcile(
        &BTreeMap::new(),
        &BTreeMap::new(),
        &[codex.clone(), agent("sess-1")],
        (&none, &none),
        1,
    );
    assert!(!main.contains_key("codex:c1"));
    assert!(main.contains_key("sess-1"));
    let exec: BTreeSet<String> = ["codex:c1".to_string()].into();
    let (main, _, _) = reconcile(
        &BTreeMap::new(),
        &BTreeMap::new(),
        &[codex],
        (&none, &exec),
        1,
    );
    assert!(main.contains_key("codex:c1"));
}

#[test]
fn session_ids_are_short_and_plain() {
    assert!(valid_id("sess-1"));
    assert!(valid_id(&"a".repeat(256)));
    assert!(!valid_id(&"a".repeat(257)));
    assert!(!valid_id(""));
    assert!(!valid_id("a\nb"));
    assert!(!valid_id("a\u{7f}b"));
    let mut state = created();
    for bad in ["", "x\ty", &"a".repeat(300)] {
        let err = apply(
            &mut state,
            Command::SetMain {
                session: bad.to_string(),
                main: false,
            },
            1,
        );
        assert!(err.is_err());
    }
    assert!(state.kept.is_empty());
}

#[test]
fn an_exited_headless_row_never_enters_main() {
    let none = BTreeSet::new();
    let mut live = agent("sess-h");
    live.headless = true;
    live.target = AgentTarget::Process(77);
    let (main, ended, _) = reconcile(
        &BTreeMap::new(),
        &BTreeMap::new(),
        &[live.clone()],
        (&none, &none),
        1,
    );
    assert!(main.is_empty());
    let mut exited = live;
    exited.headless = false;
    exited.target = AgentTarget::Resume {
        session_id: "sess-h".to_string(),
        cwd: "/work/repo".to_string(),
    };
    assert_eq!(may_be_main(&exited), Err(EXITED.to_string()));
    let (main, _, _) = reconcile(&main, &ended, &[exited], (&none, &none), 2);
    assert!(main.is_empty());
}

#[test]
fn a_main_session_that_exits_is_ended_and_leaves_after_a_day() {
    let none = BTreeSet::new();
    let mut live = agent("sess-t");
    live.target = AgentTarget::Process(77);
    let (main, ended, _) = reconcile(
        &BTreeMap::new(),
        &BTreeMap::new(),
        &[live.clone()],
        (&none, &none),
        1,
    );
    assert!(main.contains_key("sess-t"));
    live.target = AgentTarget::Resume {
        session_id: "sess-t".to_string(),
        cwd: "/work/repo".to_string(),
    };
    let (main, ended, _) = reconcile(&main, &ended, &[live.clone()], (&none, &none), 10);
    assert_eq!(ended["sess-t"], 10);
    assert!(main.contains_key("sess-t"));
    let (main, ended, _) = reconcile(&main, &ended, &[live], (&none, &none), 10 + DAY_MS);
    assert!(main.is_empty() && ended.is_empty());
}

#[test]
fn a_short_id_in_kept_becomes_the_session_id_when_the_row_shows_both() {
    let none = BTreeSet::new();
    let kept: BTreeSet<String> = ["ab12cd34".to_string()].into();
    let row = agent("sess-1");
    let (main, ended, kept) = reconcile(
        &BTreeMap::new(),
        &BTreeMap::new(),
        std::slice::from_ref(&row),
        (&kept, &none),
        1,
    );
    assert!(main.is_empty() && ended.is_empty());
    assert_eq!(kept, ["sess-1".to_string()].into());
    let main_before: BTreeMap<_, _> = [(
        "sess-1".to_string(),
        super::main_sessions::MainSession {
            id: "sess-1".to_string(),
            tool: Tool::Claude,
            cwd: "/work/repo".to_string(),
            config_dir: None,
            transcript: None,
            respawn: Vec::new(),
            since_ms: 1,
        },
    )]
    .into();
    let (main, ended, _) = reconcile(&main_before, &BTreeMap::new(), &[row], (&kept, &none), 2);
    assert!(main.is_empty() && ended.is_empty());
}
