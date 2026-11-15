use serde_json::{json, Value};

use super::{Effect, Input, LocalCmd, NodeCore, CALL_MS};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::{job_id, JobSpec, JobState};
use crate::cluster::members::Term;
use crate::cluster::testkit::id;
use crate::cluster::wire::{Msg, Origin, OriginTier, WireError};

use super::core_tests::{answers, dispatch_cmd, pair, run_locals, Hub};

fn spec(node: &str, tool: Tool, args: Value) -> JobSpec {
    JobSpec {
        node: id(node),
        tool,
        cwd: "/srv/repo".to_string(),
        args,
        secrets: Vec::new(),
    }
}

fn dispatch_as(key: &str, spec: JobSpec, tier: OriginTier) -> LocalCmd {
    LocalCmd::Dispatch {
        key: key.to_string(),
        spec,
        tier,
    }
}

fn effects_of(hub: &Hub, name: &str) -> Vec<Effect> {
    hub.seen
        .iter()
        .filter(|(n, _)| n == name)
        .map(|(_, e)| e.clone())
        .collect()
}

fn last_run(hub: &Hub, name: &str) -> (u64, String, Value) {
    run_locals(&effects_of(hub, name))
        .last()
        .cloned()
        .expect("a local run")
}

fn job_event(job: &str, state: &str) -> Msg {
    Msg::Event {
        topic: "jobs".to_string(),
        name: "job.state".to_string(),
        data: json!({"job": job, "state": state, "detail": "exit 0"}),
    }
}

fn state_of(hub: &Hub, who: &NodeId, key: &str) -> JobState {
    hub.core(who).state().expect("state").jobs[&job_id(key)].state
}

#[test]
fn a_claude_session_starts_with_the_folder_argument_the_daemon_takes() {
    let (mut hub, m5, _w1) = pair();
    let mut w1_versions = hub.core(&m5).state().expect("state").members[&id("w1")]
        .versions
        .clone()
        .expect("versions");
    w1_versions.os = "macos".to_string();
    hub.run(
        &id("w1"),
        Input::Local {
            id: 9,
            cmd: LocalCmd::SetVersions {
                versions: w1_versions,
            },
        },
    );
    hub.local(&m5, dispatch_cmd("k", "w1", Tool::Claude));
    let (_, op, args) = last_run(&hub, "w1");
    assert_eq!(op, "session.new");
    assert_eq!(args["folder"], "/srv/repo");
    assert_eq!(args["job"], json!(job_id("k")));
}

#[test]
fn an_antigravity_session_starts_a_draft_conversation() {
    let (mut hub, m5, _w1) = pair();
    hub.local(
        &m5,
        dispatch_as(
            "k",
            spec("w1", Tool::Agy, json!({"text": "hello"})),
            OriginTier::App,
        ),
    );
    let (_, op, args) = last_run(&hub, "w1");
    assert_eq!(op, "agy.send");
    assert_eq!(args["draft_folder"], "/srv/repo");
    assert_eq!(args["text"], "hello");
}

#[test]
fn a_shell_job_starts_with_its_folder_and_command() {
    let (mut hub, m5, _w1) = pair();
    hub.local(
        &m5,
        dispatch_as(
            "k",
            spec("w1", Tool::Shell, json!({"command": "printf ok"})),
            OriginTier::App,
        ),
    );
    let (_, op, args) = last_run(&hub, "w1");
    assert_eq!(op, "shell.run");
    assert_eq!(args["cwd"], "/srv/repo");
    assert_eq!(args["command"], "printf ok");
}

fn shell_job(hub: &mut Hub, m5: &NodeId, w1: &NodeId) -> u64 {
    hub.local(
        m5,
        dispatch_as(
            "k",
            spec("w1", Tool::Shell, json!({"command": "printf ok"})),
            OriginTier::App,
        ),
    );
    let (call, _, _) = last_run(hub, "w1");
    let _ = w1;
    call
}

fn started(hub: &mut Hub, w1: &NodeId, call: u64) {
    hub.run(
        w1,
        Input::LocalDone {
            call,
            result: Ok(json!({"session": "shell:j"})),
        },
    );
}

#[test]
fn a_job_event_from_its_node_finishes_the_job() {
    let (mut hub, m5, w1) = pair();
    let call = shell_job(&mut hub, &m5, &w1);
    started(&mut hub, &w1, call);
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Running);
    hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: job_event(&job_id("k"), "done"),
        },
    );
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Done);
    let job = &hub.core(&m5).state().expect("state").jobs[&job_id("k")];
    assert_eq!(job.detail.as_deref(), Some("exit 0"));
}

#[test]
fn a_job_event_may_arrive_before_the_start_reply() {
    let (mut hub, m5, w1) = pair();
    let call = shell_job(&mut hub, &m5, &w1);
    hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: job_event(&job_id("k"), "done"),
        },
    );
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Done);
    started(&mut hub, &w1, call);
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Done);
    let reply = hub
        .seen
        .iter()
        .rev()
        .find_map(|(n, e)| match e {
            Effect::Answer { result: Ok(v), .. } if n == "m5" => Some(v.clone()),
            _ => None,
        })
        .expect("answer");
    assert_eq!(reply["state"], "done");
}

#[test]
fn a_job_event_from_another_node_or_for_a_bad_state_is_ignored() {
    let (mut hub, m5, w1) = pair();
    let w2 = hub.add("w2", "linux", false);
    hub.join("m5", "w2", "linux", false);
    let call = shell_job(&mut hub, &m5, &w1);
    started(&mut hub, &w1, call);
    hub.run(
        &m5,
        Input::Msg {
            from: w2.clone(),
            msg: job_event(&job_id("k"), "done"),
        },
    );
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Running);
    for bad in ["running", "dispatching", "lost", "nonsense"] {
        hub.run(
            &m5,
            Input::Msg {
                from: w1.clone(),
                msg: job_event(&job_id("k"), bad),
            },
        );
        assert_eq!(state_of(&hub, &m5, "k"), JobState::Running, "{bad}");
    }
    hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: job_event("j-unknown", "done"),
        },
    );
    hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: job_event(&job_id("k"), "failed"),
        },
    );
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Failed);
    hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: job_event(&job_id("k"), "done"),
        },
    );
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Failed);
}

#[test]
fn a_worker_does_not_apply_a_job_event() {
    let (mut hub, m5, w1) = pair();
    let before = hub.core(&w1).state().cloned();
    hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: job_event(&job_id("k"), "done"),
        },
    );
    assert_eq!(hub.core(&w1).state().cloned(), before);
}

fn delegate_spec(node: &str) -> JobSpec {
    spec(
        node,
        Tool::Agy,
        json!({"delegate": ["agy", "--json", "--", "hi"]}),
    )
}

#[test]
fn an_agent_may_dispatch_only_a_delegate_to_a_node_that_takes_it() {
    let (mut hub, m5, w1) = pair();
    let shell = spec("w1", Tool::Shell, json!({"command": "id"}));
    let effects = hub.local(&m5, dispatch_as("a", shell, OriginTier::Agent));
    assert!(matches!(answers(&effects)[0], Err(e) if e.code == "forbidden"));
    let effects = hub.local(
        &m5,
        dispatch_as("b", delegate_spec("w1"), OriginTier::Agent),
    );
    assert!(
        matches!(answers(&effects)[0], Err(e) if e.message == "w1 doesn't take delegated jobs")
    );
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());
    hub.local(
        &m5,
        LocalCmd::SetMember {
            node: w1.clone(),
            can_be_master: None,
            client: None,
            reference: None,
            accepts_delegation: Some(true),
            headless: None,
            desired: None,
        },
    );
    hub.local(
        &m5,
        dispatch_as("b", delegate_spec("w1"), OriginTier::Agent),
    );
    let (_, op, args) = last_run(&hub, "w1");
    assert_eq!(op, "delegate.run");
    assert_eq!(args["args"], json!(["agy", "--json", "--", "hi"]));
    assert_eq!(args["job"], json!(job_id("b")));
    let sent = hub
        .seen
        .iter()
        .find_map(|(n, e)| match e {
            Effect::Send {
                msg: Msg::Call { op, origin, .. },
                ..
            } if n == "m5" && op == "delegate.run" => Some(origin.clone()),
            _ => None,
        })
        .expect("the call");
    assert_eq!(
        sent,
        Origin {
            node: m5.clone(),
            tier: OriginTier::Agent
        }
    );
}

#[test]
fn the_app_may_send_a_delegate_to_any_node() {
    let (mut hub, m5, _w1) = pair();
    hub.local(&m5, dispatch_as("b", delegate_spec("w1"), OriginTier::App));
    let (_, op, _) = last_run(&hub, "w1");
    assert_eq!(op, "delegate.run");
}

#[test]
fn a_delegates_exit_finishes_its_job_and_comes_back_in_the_reply() {
    for (exit, want) in [(0, JobState::Done), (3, JobState::Failed)] {
        let (mut hub, m5, w1) = pair();
        let key = format!("d{exit}");
        hub.local(&m5, dispatch_as(&key, delegate_spec("w1"), OriginTier::App));
        let (call, _, _) = last_run(&hub, "w1");
        hub.run(
            &w1,
            Input::LocalDone {
                call,
                result: Ok(json!({"exit": exit, "out": "hi\n", "err": "warn\n"})),
            },
        );
        assert_eq!(state_of(&hub, &m5, &key), want);
        let reply = hub
            .seen
            .iter()
            .rev()
            .find_map(|(n, e)| match e {
                Effect::Answer { result: Ok(v), .. } if n == "m5" => Some(v.clone()),
                _ => None,
            })
            .expect("answer");
        assert_eq!(reply["exit"], exit);
        assert_eq!(reply["out"], "hi\n");
        assert_eq!(reply["err"], "warn\n");
    }
}

#[test]
fn a_delegate_may_run_longer_than_a_start_call() {
    let (mut hub, m5, _w1) = pair();
    hub.local(&m5, dispatch_as("d", delegate_spec("w1"), OriginTier::App));
    hub.local(
        &m5,
        dispatch_as(
            "s",
            spec("w1", Tool::Shell, json!({"command": "id"})),
            OriginTier::App,
        ),
    );
    for _ in 0..7 {
        hub.tick_all(CALL_MS / 6);
    }
    let timeouts = hub
        .seen
        .iter()
        .filter(|(n, e)| {
            n == "m5"
                && matches!(e, Effect::Answer { result: Err(WireError { code, .. }), .. } if code == "timeout")
        })
        .count();
    assert_eq!(timeouts, 1);
    assert_eq!(state_of(&hub, &m5, "s"), JobState::Failed);
    assert_eq!(state_of(&hub, &m5, "d"), JobState::Dispatching);
}

#[test]
fn a_peers_dispatch_for_codex_is_refused() {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.join("m5", "w1", "linux", false);
    hub.link(&m1, &w1);
    let effects = hub.local(&m1, dispatch_cmd("k", "w1", Tool::Codex));
    let a = answers(&effects);
    assert!(a.is_empty());
    let reply = hub
        .seen
        .iter()
        .rev()
        .find_map(|(n, e)| match e {
            Effect::Answer { result, .. } if n == "m1" => Some(result.clone()),
            _ => None,
        })
        .expect("m1 hears back");
    assert!(
        matches!(&reply, Err(e) if e.message == "Codex jobs on another machine come in a later version"),
        "{reply:?}"
    );
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());
}

#[test]
fn a_stepped_down_home_says_who_is_primary_now() {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    let newer = Term {
        n: 2,
        node: id("m1"),
    };
    hub.run(
        &m5,
        Input::Msg {
            from: m1.clone(),
            msg: Msg::Refused {
                reason: "stale_term".to_string(),
                term: Some(newer.clone()),
                home: Some(id("m1")),
            },
        },
    );
    assert!(!hub.core(&m5).is_home());
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: m1.clone(),
            msg: Msg::Call {
                id: 77,
                op: "cluster.dispatch".to_string(),
                args: json!({"key": "k", "spec": spec("m5", Tool::Shell, json!({"command": "id"}))}),
                origin: Origin {
                    node: m1.clone(),
                    tier: OriginTier::App,
                },
                term: newer,
            },
        },
    );
    let refused = effects.iter().any(|e| {
        matches!(e, Effect::Send { msg: Msg::Reply { id: 77, err: Some(err), .. }, .. }
            if err.message == "m1 is primary now")
    });
    assert!(refused, "{effects:?}");
    assert!(NodeCore::new(
        id("x"),
        crate::cluster::testkit::versions("linux"),
        None,
        false
    )
    .state()
    .is_none());
}

#[test]
fn status_lists_jobs_newest_first_without_their_arguments() {
    let (mut hub, m5, w1) = pair();
    assert_eq!(hub.core(&m5).status()["jobs"], json!([]));
    let call = shell_job(&mut hub, &m5, &w1);
    started(&mut hub, &w1, call);
    hub.tick_all(10);
    hub.local(
        &m5,
        dispatch_as(
            "k2",
            spec("w1", Tool::Shell, json!({"command": "second"})),
            OriginTier::App,
        ),
    );
    let jobs = hub.core(&m5).status()["jobs"].clone();
    let list = jobs.as_array().expect("jobs");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["id"], json!(job_id("k2")));
    assert_eq!(list[0]["state"], "dispatching");
    assert_eq!(list[1]["id"], json!(job_id("k")));
    assert_eq!(list[1]["state"], "running");
    assert_eq!(list[1]["session"], "shell:j");
    assert_eq!(list[1]["node"], json!(w1));
    assert_eq!(list[1]["tool"], "shell");
    assert_eq!(list[1]["qualified"], json!(format!("{w1}/shell:j")));
    assert!(!jobs.to_string().contains("printf ok"));
}

#[test]
fn the_homes_own_job_hears_its_local_event() {
    let (mut hub, m5, _w1) = pair();
    hub.local(
        &m5,
        dispatch_as(
            "own",
            spec("m5", Tool::Shell, json!({"command": "printf ok"})),
            OriginTier::App,
        ),
    );
    let (call, op, _) = last_run(&hub, "m5");
    assert_eq!(op, "shell.run");
    hub.run(
        &m5,
        Input::LocalDone {
            call,
            result: Ok(json!({"session": "shell:j"})),
        },
    );
    assert_eq!(state_of(&hub, &m5, "own"), JobState::Running);
    hub.local(
        &m5,
        LocalCmd::JobEvent {
            name: "job.state".to_string(),
            data: json!({"job": job_id("own"), "state": "done"}),
        },
    );
    assert_eq!(state_of(&hub, &m5, "own"), JobState::Done);
}

fn refusal_to(hub: &Hub, who: &str) -> Result<Value, WireError> {
    hub.seen
        .iter()
        .rev()
        .find_map(|(n, e)| match e {
            Effect::Answer { result, .. } if n == who => Some(result.clone()),
            _ => None,
        })
        .expect("an answer")
}

#[test]
fn a_tool_start_is_sent_with_the_app_origin() {
    let (mut hub, m5, _w1) = pair();
    hub.local(
        &m5,
        dispatch_as(
            "k",
            spec("w1", Tool::Shell, json!({"command": "printf ok"})),
            OriginTier::App,
        ),
    );
    let sent = hub
        .seen
        .iter()
        .find_map(|(n, e)| match e {
            Effect::Send {
                msg: Msg::Call { op, origin, .. },
                ..
            } if n == "m5" && op == "shell.run" => Some(origin.clone()),
            _ => None,
        })
        .expect("the call");
    assert_eq!(
        sent,
        Origin {
            node: m5.clone(),
            tier: OriginTier::App
        }
    );
}

#[test]
fn the_core_refuses_secrets_off_the_allowlist_from_the_home_and_from_a_peer() {
    let (mut hub, m5, _w1) = pair();
    let mut with = spec("w1", Tool::Shell, json!({"command": "id"}));
    with.secrets = vec!["gh".to_string()];
    let effects = hub.local(&m5, dispatch_as("s", with.clone(), OriginTier::App));
    assert!(matches!(
        answers(&effects)[0],
        Err(e) if e.message == "gh isn't allowed on that machine"
    ));
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());

    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.join("m5", "w1", "linux", false);
    hub.link(&m1, &w1);
    hub.local(&m1, dispatch_as("s", with, OriginTier::App));
    let reply = refusal_to(&hub, "m1");
    assert!(
        matches!(&reply, Err(e) if e.message == "gh isn't allowed on that machine"),
        "{reply:?}"
    );
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());
}

#[test]
fn a_delegate_array_needs_the_agy_tool() {
    let (mut hub, m5, _w1) = pair();
    for tool in [Tool::Shell, Tool::Claude] {
        let mut odd = delegate_spec("w1");
        odd.tool = tool;
        let effects = hub.local(&m5, dispatch_as("odd", odd, OriginTier::App));
        assert!(
            matches!(answers(&effects)[0], Err(e) if e.code == "bad_args"),
            "{tool:?}"
        );
    }
    let not_a_list = spec("w1", Tool::Agy, json!({"delegate": "agy"}));
    let effects = hub.local(&m5, dispatch_as("odd", not_a_list, OriginTier::App));
    assert!(matches!(answers(&effects)[0], Err(e) if e.code == "bad_args"));
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());
}

#[test]
fn a_job_that_ended_before_its_start_reply_still_keeps_its_session() {
    let (mut hub, m5, w1) = pair();
    let call = shell_job(&mut hub, &m5, &w1);
    hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: job_event(&job_id("k"), "done"),
        },
    );
    started(&mut hub, &w1, call);
    let job = &hub.core(&m5).state().expect("state").jobs[&job_id("k")];
    assert_eq!(job.state, JobState::Done);
    assert_eq!(job.session.as_deref(), Some("shell:j"));
    let listed = hub.core(&m5).status()["jobs"][0].clone();
    assert_eq!(listed["state"], "done");
    assert_eq!(listed["session"], "shell:j");
    assert_eq!(listed["qualified"], json!(format!("{w1}/shell:j")));
}
