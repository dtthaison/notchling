use serde_json::{json, Value};

use super::{Effect, Input, LocalCmd, GRANT_ATTACH, GRANT_REVOKE_JOB, WAITING};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::{job_id, JobSpec, JobState};
use crate::cluster::testkit::id;
use crate::cluster::wire::{Msg, OriginTier, WireError};

use super::core_tests::{answers, pair, run_locals, Hub};

pub(super) fn secret_job(key: &str, names: &[&str]) -> LocalCmd {
    LocalCmd::Dispatch {
        key: key.to_string(),
        spec: JobSpec {
            node: id("w1"),
            tool: Tool::Shell,
            cwd: "/srv/repo".to_string(),
            args: json!({"command": "printf ok"}),
            secrets: names.iter().map(|n| n.to_string()).collect(),
        },
        tier: OriginTier::App,
    }
}

fn allow(hub: &mut Hub, home: &NodeId, names: &[&str], any: bool) {
    let effects = hub.local(
        home,
        LocalCmd::SetAllow {
            node: id("w1"),
            names: Some(names.iter().map(|n| n.to_string()).collect()),
            any: Some(any),
        },
    );
    assert!(answers(&effects)[0].is_ok(), "{:?}", answers(&effects));
}

fn state_of(hub: &Hub, who: &NodeId, key: &str) -> JobState {
    hub.core(who).state().expect("state").jobs[&job_id(key)].state
}

fn detail_of(hub: &Hub, who: &NodeId, key: &str) -> Option<String> {
    hub.core(who).state().expect("state").jobs[&job_id(key)]
        .detail
        .clone()
}

fn runs(effects: &[Effect], op: &str) -> Vec<(u64, Value)> {
    run_locals(effects)
        .into_iter()
        .filter(|(_, name, _)| name == op)
        .map(|(call, _, args)| (call, args))
        .collect()
}

fn sent_calls(effects: &[Effect], op: &str) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Send { msg: Msg::Call { op: name, .. }, .. } if name == op))
        .count()
}

fn done(hub: &mut Hub, who: &NodeId, call: u64, result: Result<Value, WireError>) -> Vec<Effect> {
    hub.run(who, Input::LocalDone { call, result })
}

fn worker_start(hub: &Hub) -> u64 {
    let seen: Vec<Effect> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "w1")
        .map(|(_, e)| e.clone())
        .collect();
    runs(&seen, "shell.run").pop().expect("the start").0
}

#[test]
fn a_name_off_the_allowlist_is_refused_before_a_job_is_recorded() {
    let (mut hub, m5, _w1) = pair();
    let effects = hub.local(&m5, secret_job("k", &["gh"]));
    assert!(
        matches!(answers(&effects)[0], Err(e) if e.message == "gh isn't allowed on that machine")
    );
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());
    assert!(runs(&effects, GRANT_ATTACH).is_empty());
    allow(&mut hub, &m5, &["other"], false);
    let effects = hub.local(&m5, secret_job("k", &["gh"]));
    assert!(answers(&effects)[0].is_err());
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());
}

#[test]
fn an_allowed_secret_is_attached_before_the_tool_starts() {
    let (mut hub, m5, w1) = pair();
    allow(&mut hub, &m5, &["gh"], false);
    let effects = hub.local(&m5, secret_job("k", &["GH"]));
    let attach = runs(&effects, GRANT_ATTACH);
    assert_eq!(attach.len(), 1);
    assert_eq!(attach[0].1["job"]["id"], json!(job_id("k")));
    assert_eq!(attach[0].1["vaultPrimary"], true);
    assert_eq!(sent_calls(&effects, "shell.run"), 0);
    assert!(answers(&effects).is_empty());
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Dispatching);
    let effects = done(&mut hub, &m5, attach[0].0, Ok(json!({"delivered": ["gh"]})));
    assert_eq!(sent_calls(&effects, "shell.run"), 1);
    let start = worker_start(&hub);
    done(&mut hub, &w1, start, Ok(json!({"session": "shell:j"})));
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Running);
}

#[test]
fn a_refused_attach_fails_the_job_with_the_reason_and_revokes() {
    let (mut hub, m5, _w1) = pair();
    allow(&mut hub, &m5, &["gh"], false);
    let effects = hub.local(&m5, secret_job("k", &["gh"]));
    let (call, _) = runs(&effects, GRANT_ATTACH).remove(0);
    let reason = WireError::new("needs_approval", "The user didn't allow it.");
    let effects = done(&mut hub, &m5, call, Err(reason));
    assert!(matches!(
        answers(&effects)[0],
        Err(e) if e.message == "The user didn't allow it."
    ));
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Failed);
    assert_eq!(
        detail_of(&hub, &m5, "k").as_deref(),
        Some("The user didn't allow it.")
    );
    let revoked = runs(&effects, GRANT_REVOKE_JOB);
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0].1["job"], json!(job_id("k")));
    assert_eq!(revoked[0].1["node"], json!(id("w1")));
}

#[test]
fn a_waiting_attach_keeps_the_job_dispatching_and_answers_at_once() {
    let (mut hub, m5, _w1) = pair();
    allow(&mut hub, &m5, &["gh"], false);
    let effects = hub.local(&m5, secret_job("k", &["gh"]));
    let (call, _) = runs(&effects, GRANT_ATTACH).remove(0);
    let effects = done(
        &mut hub,
        &m5,
        call,
        Ok(json!({"waiting": "m5, which holds the vault"})),
    );
    let answered = answers(&effects);
    assert!(matches!(answered[0], Ok(v) if v["state"] == "dispatching"));
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Dispatching);
    let detail = detail_of(&hub, &m5, "k").expect("a detail");
    assert!(detail.starts_with(WAITING), "{detail}");
    assert_eq!(detail, "Waiting for m5, which holds the vault");
    assert_eq!(sent_calls(&effects, "shell.run"), 0);
}

#[test]
fn a_waiting_job_is_attached_again_when_the_vault_node_is_primary() {
    let (mut hub, m5, w1) = pair();
    allow(&mut hub, &m5, &["gh"], false);
    let effects = hub.local(&m5, secret_job("k", &["gh"]));
    let (call, _) = runs(&effects, GRANT_ATTACH).remove(0);
    done(
        &mut hub,
        &m5,
        call,
        Ok(json!({"waiting": "m5, which holds the vault"})),
    );
    let effects = hub.tick(&m5, 1_000);
    let again = runs(&effects, GRANT_ATTACH);
    assert_eq!(again.len(), 1);
    let quiet = hub.tick(&m5, 1_000);
    assert!(runs(&quiet, GRANT_ATTACH).is_empty());
    let effects = done(&mut hub, &m5, again[0].0, Ok(json!({"delivered": ["gh"]})));
    assert_eq!(sent_calls(&effects, "shell.run"), 1);
    let start = worker_start(&hub);
    done(&mut hub, &w1, start, Ok(json!({"session": "shell:j"})));
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Running);
    assert_eq!(detail_of(&hub, &m5, "k"), None);
}

#[test]
fn nothing_is_attached_while_another_node_is_primary() {
    let mut hub = Hub::new();
    let _m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.join("m5", "w1", "linux", false);
    hub.link(&m1, &w1);
    hub.nodes.get_mut(&m1).expect("m1").bump_term();
    assert!(hub.core(&m1).is_home());
    let effects = hub.local(
        &m1,
        LocalCmd::SetAllow {
            node: w1.clone(),
            names: Some(vec!["gh".to_string()]),
            any: None,
        },
    );
    assert!(answers(&effects)[0].is_ok());
    let effects = hub.local(&m1, secret_job("k", &["gh"]));
    let attach = runs(&effects, GRANT_ATTACH);
    assert_eq!(attach.len(), 1);
    assert_eq!(attach[0].1["vaultPrimary"], false);
    assert_eq!(attach[0].1["vaultName"], "m5");
    done(
        &mut hub,
        &m1,
        attach[0].0,
        Ok(json!({"waiting": "m5, which holds the vault"})),
    );
    let quiet = hub.tick(&m1, 1_000);
    assert!(runs(&quiet, GRANT_ATTACH).is_empty());
    assert_eq!(state_of(&hub, &m1, "k"), JobState::Dispatching);
}

#[test]
fn a_job_on_the_vault_node_takes_no_attached_secret() {
    let (mut hub, m5, _w1) = pair();
    let spec = JobSpec {
        node: m5.clone(),
        tool: Tool::Shell,
        cwd: "/srv/repo".to_string(),
        args: json!({"command": "id"}),
        secrets: vec!["gh".to_string()],
    };
    let effects = hub.local(
        &m5,
        LocalCmd::Dispatch {
            key: "k".to_string(),
            spec,
            tier: OriginTier::App,
        },
    );
    assert!(matches!(
        answers(&effects)[0],
        Err(e) if e.message.contains("holds the vault")
    ));
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());
}

#[test]
fn a_delegate_job_takes_no_secrets_and_the_list_is_capped() {
    let (mut hub, m5, _w1) = pair();
    allow(&mut hub, &m5, &[], true);
    let delegate = LocalCmd::Dispatch {
        key: "d".to_string(),
        spec: JobSpec {
            node: id("w1"),
            tool: Tool::Agy,
            cwd: "/srv".to_string(),
            args: json!({"delegate": ["--model", "x"]}),
            secrets: vec!["gh".to_string()],
        },
        tier: OriginTier::App,
    };
    let effects = hub.local(&m5, delegate);
    assert!(matches!(answers(&effects)[0], Err(e) if e.code == "bad_args"));
    let many: Vec<String> = (0..17).map(|n| format!("s{n}")).collect();
    let names: Vec<&str> = many.iter().map(String::as_str).collect();
    let effects = hub.local(&m5, secret_job("many", &names));
    assert!(matches!(answers(&effects)[0], Err(e) if e.code == "bad_args"));
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());
}

#[test]
fn a_finished_job_revokes_its_grants() {
    let (mut hub, m5, w1) = pair();
    allow(&mut hub, &m5, &["gh"], false);
    let effects = hub.local(&m5, secret_job("k", &["gh"]));
    let (call, _) = runs(&effects, GRANT_ATTACH).remove(0);
    done(&mut hub, &m5, call, Ok(json!({"delivered": ["gh"]})));
    let start = worker_start(&hub);
    done(&mut hub, &w1, start, Ok(json!({"session": "shell:j"})));
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Event {
                topic: "jobs".to_string(),
                name: "job.state".to_string(),
                data: json!({"job": job_id("k"), "state": "done", "detail": "exit 0"}),
            },
        },
    );
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Done);
    assert_eq!(runs(&effects, GRANT_REVOKE_JOB).len(), 1);
}

#[test]
fn a_failed_start_after_delivery_revokes_the_grants() {
    let (mut hub, m5, w1) = pair();
    allow(&mut hub, &m5, &["gh"], false);
    let effects = hub.local(&m5, secret_job("k", &["gh"]));
    let (call, _) = runs(&effects, GRANT_ATTACH).remove(0);
    done(&mut hub, &m5, call, Ok(json!({"delivered": ["gh"]})));
    let start = worker_start(&hub);
    done(
        &mut hub,
        &w1,
        start,
        Err(WireError::new("failed", "no shell")),
    );
    assert_eq!(state_of(&hub, &m5, "k"), JobState::Failed);
    let revoked: Vec<_> = hub
        .seen
        .iter()
        .filter(|(n, e)| {
            n == "m5" && matches!(e, Effect::RunLocal { op, .. } if op == GRANT_REVOKE_JOB)
        })
        .collect();
    assert_eq!(revoked.len(), 1);
}

#[test]
fn the_allowlist_is_replaced_by_names_and_any_and_relayed_from_a_client() {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.join("m5", "w1", "linux", false);
    hub.link(&m1, &w1);
    let effects = hub.local(
        &m1,
        LocalCmd::SetAllow {
            node: w1.clone(),
            names: Some(vec!["GH".to_string(), "npm".to_string()]),
            any: None,
        },
    );
    assert!(effects.iter().any(
        |e| matches!(e, Effect::Send { msg: Msg::Call { op, .. }, .. } if op == "cluster.allow.set")
    ));
    let secrets = &hub.core(&m5).state().expect("state").members[&w1].secrets;
    assert_eq!(
        secrets.names.iter().cloned().collect::<Vec<_>>(),
        vec!["gh".to_string(), "npm".to_string()]
    );
    assert!(!secrets.any);
    hub.local(
        &m5,
        LocalCmd::SetAllow {
            node: w1.clone(),
            names: None,
            any: Some(true),
        },
    );
    let secrets = &hub.core(&m5).state().expect("state").members[&w1].secrets;
    assert!(secrets.any && secrets.names.len() == 2);
    let status = hub.core(&m5).status();
    let listed = status["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|m| m["id"] == json!(w1))
        .expect("w1")["secrets"]
        .clone();
    assert_eq!(listed["any"], true);
}

#[test]
fn a_bad_allowlist_name_changes_nothing() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.local(
        &m5,
        LocalCmd::SetAllow {
            node: w1.clone(),
            names: Some(vec!["../x".to_string()]),
            any: None,
        },
    );
    assert!(answers(&effects)[0].is_err());
    let secrets = &hub.core(&m5).state().expect("state").members[&w1].secrets;
    assert!(secrets.names.is_empty() && !secrets.any);
}
