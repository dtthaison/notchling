use std::collections::BTreeSet;

use super::core_tests::{answers, pair, Hub};
use super::Effect;
use super::LocalCmd;
use crate::cluster::testkit::id;
use crate::sessions::{Agent, AgentGroup, AgentTarget};

fn agent(id: &str) -> Agent {
    Agent {
        id: id.to_string(),
        name: id.to_string(),
        project: "repo".to_string(),
        cwd: "/work/repo".to_string(),
        group: AgentGroup::Working,
        label: "Working".to_string(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job("ab12cd34".to_string()),
        suggested_reply: None,
        transcript_path: Some(format!("/t/{id}.jsonl")),
        host: "Background".to_string(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
    }
}

fn client_pair() -> Hub {
    let mut hub = Hub::new();
    hub.add("m5", "macos", true);
    hub.add("c1", "macos", true);
    hub.create("m5");
    hub.join_as("m5", "c1", "macos", true, false);
    hub
}

#[test]
fn the_home_keeps_main_current_from_the_session_list() {
    let mut hub = client_pair();
    let m5 = id("m5");
    hub.local(
        &m5,
        LocalCmd::MainSeen {
            agents: vec![agent("job-1"), agent("agy:x"), agent("codex:other")],
            codex_exec: BTreeSet::new(),
        },
    );
    let state = hub.core(&m5).state().expect("state");
    assert!(state.main.contains_key("job-1"));
    assert!(!state.main.contains_key("agy:x"));
    assert!(!state.main.contains_key("codex:other"));
}

#[test]
fn a_member_that_is_not_the_home_ignores_the_session_list() {
    let mut hub = client_pair();
    let c1 = id("c1");
    hub.local(
        &c1,
        LocalCmd::MainSeen {
            agents: vec![agent("job-1")],
            codex_exec: BTreeSet::new(),
        },
    );
    assert!(hub.core(&c1).state().expect("state").main.is_empty());
}

#[test]
fn keep_on_reaches_the_home_from_a_client() {
    let mut hub = client_pair();
    let (m5, c1) = (id("m5"), id("c1"));
    hub.local(
        &m5,
        LocalCmd::MainSeen {
            agents: vec![agent("job-1")],
            codex_exec: BTreeSet::new(),
        },
    );
    hub.local(
        &c1,
        LocalCmd::SetMain {
            session: "job-1".to_string(),
            main: false,
        },
    );
    let home = hub.core(&m5).state().expect("state");
    assert!(home.kept.contains("job-1"));
    assert!(!home.main.contains_key("job-1"));
    hub.local(
        &c1,
        LocalCmd::SetMain {
            session: "job-1".to_string(),
            main: true,
        },
    );
    assert!(hub.core(&m5).state().expect("state").kept.is_empty());
}

#[test]
fn a_worker_cannot_change_main_sessions() {
    let (mut hub, m5, w1) = pair();
    hub.local(
        &w1,
        LocalCmd::SetMain {
            session: "job-1".to_string(),
            main: false,
        },
    );
    assert!(hub.core(&m5).state().expect("state").kept.is_empty());
    assert!(hub.seen.iter().any(|(_, e)| matches!(
        e,
        Effect::Publish { name, data, .. }
            if name == "cluster.refused" && data["op"] == "cluster.main.set"
    )));
}

#[test]
fn the_home_applies_a_local_set_main_at_once() {
    let mut hub = client_pair();
    let m5 = id("m5");
    let effects = hub.local(
        &m5,
        LocalCmd::SetMain {
            session: "job-1".to_string(),
            main: false,
        },
    );
    assert!(answers(&effects).iter().all(|r| r.is_ok()));
    assert!(hub.core(&m5).state().expect("state").kept.contains("job-1"));
}
