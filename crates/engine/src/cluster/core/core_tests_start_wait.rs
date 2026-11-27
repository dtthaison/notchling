use serde_json::{json, Value};

use super::core_tests::{pair, Hub};
use super::{Input, LocalCmd, CALL_MS};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::{job_id, JobSpec, JobState};
use crate::cluster::testkit::id;
use crate::cluster::wire::OriginTier;

const STEP: u64 = CALL_MS / 6;

fn dispatch(hub: &mut Hub, m5: &NodeId, key: &str, tool: Tool, args: Value) {
    hub.local(
        m5,
        LocalCmd::Dispatch {
            key: key.to_string(),
            spec: JobSpec {
                node: id("w1"),
                tool,
                cwd: "/srv/repo".to_string(),
                args,
                secrets: Vec::new(),
            },
            tier: OriginTier::App,
        },
    );
}

fn macos_worker(hub: &mut Hub, m5: &NodeId) {
    let mut versions = hub.core(m5).state().expect("state").members[&id("w1")]
        .versions
        .clone()
        .expect("versions");
    versions.os = "macos".to_string();
    hub.run(
        &id("w1"),
        Input::Local {
            id: 9,
            cmd: LocalCmd::SetVersions { versions },
        },
    );
}

fn state_of(hub: &Hub, m5: &NodeId, key: &str) -> JobState {
    hub.core(m5).state().expect("state").jobs[&job_id(key)].state
}

fn wait(hub: &mut Hub, seconds: u64) {
    for _ in 0..(seconds * 1000 / STEP) {
        hub.tick_all(STEP);
    }
}

#[test]
fn a_claude_start_may_take_longer_than_a_plain_call_and_still_ends_in_time() {
    let (mut hub, m5, _w1) = pair();
    macos_worker(&mut hub, &m5);
    dispatch(&mut hub, &m5, "c", Tool::Claude, json!({}));
    dispatch(&mut hub, &m5, "s", Tool::Shell, json!({"command": "id"}));
    wait(&mut hub, 40);
    assert_eq!(state_of(&hub, &m5, "s"), JobState::Failed);
    assert_eq!(state_of(&hub, &m5, "c"), JobState::Dispatching);
    wait(&mut hub, 70);
    assert_eq!(state_of(&hub, &m5, "c"), JobState::Failed);
}

#[test]
fn a_claude_start_that_answers_after_a_minute_still_runs() {
    let (mut hub, m5, w1) = pair();
    macos_worker(&mut hub, &m5);
    dispatch(&mut hub, &m5, "c", Tool::Claude, json!({}));
    let call = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "w1")
        .filter_map(|(_, e)| match e {
            super::Effect::RunLocal { call, .. } => Some(*call),
            _ => None,
        })
        .next_back()
        .expect("a local run");
    wait(&mut hub, 65);
    hub.run(
        &w1,
        Input::LocalDone {
            call,
            result: Ok(json!({"jobId": "j-1"})),
        },
    );
    assert_eq!(state_of(&hub, &m5, "c"), JobState::Running);
}
