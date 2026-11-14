use serde_json::json;

use super::{Effect, Input, LocalCmd, NodeCore, OFFLINE_MS};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::JobState;
use crate::cluster::testkit::{fp, id, versions};
use crate::cluster::wire::{Msg, WireError};

use super::core_tests::{answers, dispatch_cmd, pair, run_locals, Hub};

#[test]
fn dispatch_to_offline_node_is_refused() {
    let (mut hub, m5, w1) = pair();
    hub.tick(&m5, OFFLINE_MS);
    assert!(!hub.core(&m5).online().contains(&w1));
    let effects = hub.local(&m5, dispatch_cmd("k", "w1", Tool::Codex));
    let a = answers(&effects);
    assert!(matches!(a[0], Err(e) if e.message == "w1 is offline"));
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());
}

#[test]
fn dispatch_runs_once_and_moves_the_job_to_running() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.local(&m5, dispatch_cmd("k", "w1", Tool::Codex));
    assert!(answers(&effects).is_empty());
    let w1_effects: Vec<Effect> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "w1")
        .map(|(_, e)| e.clone())
        .collect();
    let runs = run_locals(&w1_effects);
    let (call, op, args) = runs.last().cloned().expect("run");
    assert_eq!(op, "codex.send");
    assert_eq!(args["draft_folder"], "/srv/repo");
    let job = crate::cluster::jobs::job_id("k");
    assert_eq!(args["job"], job);
    let done = hub.run(
        &w1,
        Input::LocalDone {
            call,
            result: Ok(json!({"id": "codex:abc"})),
        },
    );
    assert!(!done.is_empty());
    let state = hub.core(&m5).state().expect("state");
    assert_eq!(state.jobs[&job].state, JobState::Running);
    assert_eq!(state.jobs[&job].session.as_deref(), Some("codex:abc"));
    let answered = hub
        .seen
        .iter()
        .filter(|(n, e)| {
            n == "m5" && matches!(e, Effect::Answer { result: Ok(v), .. } if v["job"] == json!(job))
        })
        .count();
    assert_eq!(answered, 1);
    let again = hub.local(&m5, dispatch_cmd("k", "w1", Tool::Codex));
    assert!(matches!(answers(&again)[0], Ok(v) if v["job"] == json!(job)));
    assert_eq!(
        hub.seen
            .iter()
            .filter(|(n, e)| n == "w1" && matches!(e, Effect::RunLocal { .. }))
            .count(),
        1
    );
}

#[test]
fn a_failed_start_fails_the_job() {
    let (mut hub, m5, w1) = pair();
    hub.local(&m5, dispatch_cmd("k", "w1", Tool::Shell));
    let w1_effects: Vec<Effect> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "w1")
        .map(|(_, e)| e.clone())
        .collect();
    let (call, op, _) = run_locals(&w1_effects).last().cloned().expect("run");
    assert_eq!(op, "shell.run");
    hub.run(
        &w1,
        Input::LocalDone {
            call,
            result: Err(WireError::new("failed", "no shell")),
        },
    );
    let job = crate::cluster::jobs::job_id("k");
    let state = hub.core(&m5).state().expect("state");
    assert_eq!(state.jobs[&job].state, JobState::Failed);
    assert_eq!(state.jobs[&job].detail.as_deref(), Some("no shell"));
}

#[test]
fn dispatch_from_client_goes_through_home() {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.join("m5", "w1", "linux", false);
    assert!(hub.core(&m5).state().expect("state").members[&m1].client);
    hub.link(&m1, &w1);
    hub.tick_all(0);
    let effects = hub.local(&m1, dispatch_cmd("k", "w1", Tool::Codex));
    assert!(answers(&effects).is_empty());
    let calls: Vec<_> = effects
        .iter()
        .filter(|e| matches!(e, Effect::Send { to, msg: Msg::Call { op, .. } } if *to == m5 && op == "cluster.dispatch"))
        .collect();
    assert_eq!(calls.len(), 1);
    let w1_effects: Vec<Effect> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "w1")
        .map(|(_, e)| e.clone())
        .collect();
    let (call, _, _) = run_locals(&w1_effects).last().cloned().expect("run");
    hub.run(
        &w1,
        Input::LocalDone {
            call,
            result: Ok(json!({"id": "s"})),
        },
    );
    let answered = hub
        .seen
        .iter()
        .filter(|(n, e)| {
            n == "m1"
                && matches!(e, Effect::Answer { result: Ok(v), .. } if v["state"] == "running")
        })
        .count();
    assert_eq!(answered, 1);
}

#[test]
fn tool_mismatch_dispatches_with_a_warning() {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    hub.create("m5");
    hub.join("m5", "m1", "macos", true);
    let mut older = versions("macos");
    older.tools.insert(
        crate::versions::Tool::Claude,
        crate::versions::Version::parse("2.1.323").expect("version"),
    );
    hub.run(
        &m1,
        Input::Local {
            id: 7,
            cmd: LocalCmd::SetVersions { versions: older },
        },
    );
    assert!(hub.core(&m5).state().expect("state").members[&m1]
        .versions
        .as_ref()
        .is_some_and(|v| v.tools[&crate::versions::Tool::Claude].as_str() == "2.1.323"));
    hub.local(&m5, dispatch_cmd("k", "m1", Tool::Claude));
    let m1_effects: Vec<Effect> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "m1")
        .map(|(_, e)| e.clone())
        .collect();
    let (call, op, _) = run_locals(&m1_effects).last().cloned().expect("run");
    assert_eq!(op, "session.new");
    hub.run(
        &m1,
        Input::LocalDone {
            call,
            result: Ok(json!({"session": "s1"})),
        },
    );
    let reply = hub
        .seen
        .iter()
        .rev()
        .find_map(|(n, e)| match e {
            Effect::Answer { result: Ok(v), .. } if n == "m5" => Some(v.clone()),
            _ => None,
        })
        .expect("answer");
    assert_eq!(
        reply["warning"],
        "m1 runs Claude Code 2.1.323, the Mac 2.1.324"
    );
}

#[test]
fn a_refusing_compat_stops_the_dispatch() {
    let (mut hub, m5, _w1) = pair();
    let effects = hub.local(&m5, dispatch_cmd("k", "w1", Tool::Claude));
    let a = answers(&effects);
    assert!(matches!(a[0], Err(e) if e.message.contains("isn't proven yet")));
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());
}

#[test]
fn dispatch_to_the_home_itself_runs_locally() {
    let (mut hub, m5, _) = pair();
    let effects = hub.local(&m5, dispatch_cmd("k", "m5", Tool::Codex));
    assert_eq!(run_locals(&effects).len(), 1);
}

#[test]
fn status_lists_roles_and_online() {
    let (hub, m5, w1) = pair();
    let status = hub.core(&m5).status();
    assert_eq!(status["cluster"]["home"], json!(m5));
    let members = status["members"].as_array().expect("members");
    let row = |id: &NodeId| {
        members
            .iter()
            .find(|m| m["id"] == json!(id))
            .expect("row")
            .clone()
    };
    assert_eq!(row(&m5)["role"], "home");
    assert_eq!(row(&w1)["role"], "worker");
    assert_eq!(row(&w1)["online"], true);
    let empty = NodeCore::new(id("x1"), versions("linux"), None, false).status();
    assert!(empty["cluster"].is_null());
}

#[test]
fn a_second_join_token_use_by_an_active_member_gets_its_answer_again() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Join {
                secret: "spent".to_string(),
                name: "w1".to_string(),
                fingerprint: fp("w1"),
                addrs: vec![],
                port: 1,
                versions: versions("linux"),
            },
        },
    );
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Send {
            msg: Msg::JoinOk { .. },
            ..
        }
    )));
}
