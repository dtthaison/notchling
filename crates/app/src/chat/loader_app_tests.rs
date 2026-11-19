use std::path::Path;

use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};

use super::data::{DataRoot, Tool};
use super::loader::Loader;
use crate::testutil::Scratch;
use crate::tools::live::ToolLive;

const OURS: &str = "agy:79ba6320-1b32-46fe-838d-9bb614eb992d";
const TEST: &str = "agy:457369a9-35a1-40f8-92b9-26b6398b78a1";
const NATIVE: &str = "agy:bf68ea82-bc31-4c24-b8bf-8aa150aa05d4";
const CLI: &str = "agy:9ddb7ec8-ff66-4511-b25a-0ad5a8ed8c8e";
const FRESH: &str = "agy:5ce2bd47-0c1e-4e6b-8f43-6aa0e2f1d3aa";

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a folder");
    for entry in std::fs::read_dir(from).expect("a folder").flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("a copy");
        }
    }
}

fn world(tag: &str) -> (Scratch, Loader) {
    let s = Scratch::new(tag);
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/fixtures/agy-app"),
        s.path(),
    );
    let loader = Loader::new(DataRoot::fixture(s.path()));
    (s, loader)
}

fn row(id: &str, name: &str, group: AgentGroup) -> Agent {
    Agent {
        id: id.into(),
        name: name.into(),
        project: String::new(),
        cwd: String::new(),
        group,
        label: "Working".into(),
        detail: None,
        since_ms: 5_000,
        target: AgentTarget::Resume {
            session_id: id.trim_start_matches("agy:").into(),
            cwd: String::new(),
        },
        suggested_reply: None,
        transcript_path: None,
        host: "Antigravity".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: None,
    }
}

fn changed(live: &mut ToolLive, agent: &Agent, change: &str) {
    live.apply(&notchling_local_api::Event {
        event: "session.changed".into(),
        data: serde_json::json!({"id": agent.id, "row": agent, "change": change}),
    });
}

fn ids(loaded: &super::loader::Loaded) -> Vec<String> {
    let mut ids: Vec<String> = loaded.sessions.iter().map(|s| s.agent.id.clone()).collect();
    ids.sort();
    ids
}

#[test]
fn the_sidecars_app_conversations_are_in_the_sidebar_beside_the_cli_ones() {
    let (_s, mut loader) = world("loader-app-list");
    let loaded = loader.load(None, 1_000);
    let mut want = vec![CLI.to_owned(), OURS.to_owned(), TEST.to_owned()];
    want.sort();
    assert_eq!(ids(&loaded), want);
    assert!(!ids(&loaded).contains(&NATIVE.to_owned()));
    let ours = loaded.sessions.iter().find(|s| s.agent.id == OURS).unwrap();
    assert_eq!(ours.tool, Tool::Antigravity);
    assert_eq!(ours.agent.name, "pong");
    assert_eq!(ours.agent.group, AgentGroup::Completed);
    let cli = loaded.sessions.iter().find(|s| s.agent.id == CLI).unwrap();
    assert_eq!(cli.agent.name, "Check one illustration");
    assert_eq!(cli.agent.cwd, "/Users/me/work/shop");
}

#[test]
fn without_an_app_store_the_list_is_the_cli_list_alone() {
    let (s, mut loader) = world("loader-app-none");
    std::fs::remove_dir_all(s.path().join("antigravity")).expect("remove");
    let loaded = loader.load(None, 1_000);
    assert_eq!(ids(&loaded), [CLI]);
    assert_eq!(loaded.sessions[0].agent.name, "Check one illustration");
}

#[test]
fn a_conversation_just_started_through_the_sidecar_stays_selected_among_other_rows() {
    let (_s, mut loader) = world("loader-app-fresh");
    let mut live = ToolLive::default();
    changed(
        &mut live,
        &row(FRESH, "Build the thing", AgentGroup::Working),
        "turn_started",
    );
    let loaded = loader.load_live(Some(FRESH), None, &live, 1_000);
    assert_eq!(loaded.selected.as_deref(), Some(FRESH));
    let fresh = loaded
        .sessions
        .iter()
        .find(|s| s.agent.id == FRESH)
        .expect("the new row");
    assert_eq!(
        (fresh.tool, fresh.agent.group),
        (Tool::Antigravity, AgentGroup::Working)
    );
    assert_eq!(fresh.agent.name, "Build the thing");
    assert_eq!(loaded.sessions.len(), 4);
    assert!(live.is_open(FRESH) && live.is_working(FRESH));
}

#[test]
fn once_the_app_lists_the_started_conversation_it_is_one_row_not_two() {
    let (_s, mut loader) = world("loader-app-once");
    let mut live = ToolLive::default();
    changed(
        &mut live,
        &row(OURS, "pong", AgentGroup::Working),
        "turn_started",
    );
    let loaded = loader.load_live(Some(OURS), None, &live, 1_000);
    assert_eq!(loaded.selected.as_deref(), Some(OURS));
    assert_eq!(
        loaded
            .sessions
            .iter()
            .filter(|s| s.agent.id == OURS)
            .count(),
        1
    );
    let ours = loaded.sessions.iter().find(|s| s.agent.id == OURS).unwrap();
    assert_eq!(
        ours.agent.group,
        AgentGroup::Working,
        "the daemon's turn shows"
    );
}
