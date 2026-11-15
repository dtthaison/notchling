use serde_json::json;

use super::{
    Effect, Input, LocalCmd, ATTACH_CALL_MS, CALL_MS, GRANT_ATTACH, GRANT_REVOKE_JOB, PING_MS,
};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::JobSpec;
use crate::cluster::jobs::{job_id, JobState};
use crate::cluster::members::{MemberState, Term};
use crate::cluster::testkit::id;
use crate::cluster::wire::{Msg, Origin, OriginTier};

use super::core_tests::{answers, pair, run_locals, Hub};

fn secret_job(key: &str, names: &[&str]) -> LocalCmd {
    super::core_tests_secrets::secret_job(key, names)
}

fn allow(hub: &mut Hub, home: &NodeId, names: &[&str]) {
    let effects = hub.local(
        home,
        LocalCmd::SetAllow {
            node: id("w1"),
            names: Some(names.iter().map(|n| n.to_string()).collect()),
            any: Some(false),
        },
    );
    assert!(answers(&effects)[0].is_ok());
}

fn attach_call(effects: &[Effect]) -> u64 {
    run_locals(effects)
        .into_iter()
        .find(|(_, op, _)| op == GRANT_ATTACH)
        .expect("an attach")
        .0
}

fn from_vault(term: Term) -> Msg {
    Msg::Call {
        id: 40,
        op: "grant.deliver".to_string(),
        args: json!({"job": "j-1", "name": "gh", "value": "x", "expiresMs": 1}).into(),
        origin: Origin {
            node: id("m5"),
            tier: OriginTier::Node,
        },
        term,
    }
}

#[test]
fn a_newer_term_naming_another_home_is_adopted_before_the_sender_is_judged() {
    let (mut hub, m5, w1) = pair();
    let now = hub.core(&w1).highest_term().cloned().expect("term");
    let effects = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: from_vault(now.clone()),
        },
    );
    assert_eq!(
        run_locals(&effects)
            .iter()
            .filter(|(_, op, _)| op == "grant.deliver")
            .count(),
        1
    );
    let (mut hub, m5, w1) = pair();
    let newer = Term {
        n: now.n + 1,
        node: id("m1"),
    };
    let effects = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: from_vault(newer),
        },
    );
    assert!(run_locals(&effects).is_empty());
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Send { msg: Msg::Reply { err: Some(e), .. }, .. } if e.code == "forbidden"
    )));
}

#[test]
fn every_job_revokes_at_its_end_whether_or_not_it_named_secrets() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.local(&m5, secret_job("plain", &[]));
    assert!(run_locals(&effects)
        .iter()
        .all(|(_, op, _)| op != GRANT_ATTACH));
    let start = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "w1")
        .flat_map(|(_, e)| run_locals(std::slice::from_ref(e)))
        .find(|(_, op, _)| op == "shell.run")
        .expect("the start")
        .0;
    hub.run(
        &w1,
        Input::LocalDone {
            call: start,
            result: Ok(json!({"session": "shell:j"})),
        },
    );
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Event {
                topic: "jobs".to_string(),
                name: "job.state".to_string(),
                data: json!({"job": job_id("plain"), "state": "done", "detail": "exit 0"}),
            },
        },
    );
    let revoked: Vec<_> = run_locals(&effects)
        .into_iter()
        .filter(|(_, op, _)| op == GRANT_REVOKE_JOB)
        .collect();
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0].2["job"], json!(job_id("plain")));
}

#[test]
fn an_attach_that_finishes_after_stepping_down_starts_nothing_and_revokes() {
    let (mut hub, m5, w1) = pair();
    allow(&mut hub, &m5, &["gh"]);
    let effects = hub.local(&m5, secret_job("k", &["gh"]));
    let call = attach_call(&effects);
    let newer = Term {
        n: hub.core(&m5).highest_term().expect("term").n + 1,
        node: w1.clone(),
    };
    hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Call {
                id: 7,
                op: "node.load".to_string(),
                args: json!({}).into(),
                origin: Origin {
                    node: w1.clone(),
                    tier: OriginTier::Node,
                },
                term: newer,
            },
        },
    );
    assert!(!hub.core(&m5).is_home());
    let effects = hub.run(
        &m5,
        Input::LocalDone {
            call,
            result: Ok(json!({"delivered": ["gh"]})),
        },
    );
    assert!(!effects
        .iter()
        .any(|e| matches!(e, Effect::Send { msg: Msg::Call { op, .. }, .. } if op == "shell.run")));
    assert_eq!(
        run_locals(&effects)
            .iter()
            .filter(|(_, op, _)| op == GRANT_REVOKE_JOB)
            .count(),
        1
    );
    assert_eq!(
        hub.core(&m5).state().expect("state").jobs[&job_id("k")].state,
        JobState::Dispatching
    );
}

#[test]
fn leaving_the_cluster_forgets_the_attaches_in_flight() {
    let (mut hub, m5, w1) = pair();
    allow(&mut hub, &m5, &["gh"]);
    hub.local(&m5, secret_job("k", &["gh"]));
    assert!(!hub.core(&m5).local_runs.is_empty());
    let mut members: Vec<_> = hub
        .core(&m5)
        .state()
        .expect("state")
        .members
        .values()
        .cloned()
        .collect();
    for m in &mut members {
        if m.id == m5 {
            m.state = MemberState::Removed;
        }
    }
    let term = Term {
        n: hub.core(&m5).highest_term().expect("term").n + 1,
        node: w1.clone(),
    };
    hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Members {
                term,
                home: w1.clone(),
                vault: Some(m5.clone()),
                members,
            },
        },
    );
    assert!(hub.core(&m5).state().is_none());
    assert!(hub.core(&m5).local_runs.is_empty());
}

#[test]
fn a_grant_request_waits_as_long_as_an_approval_does() {
    let (mut hub, m5, w1) = pair();
    let origin = Origin {
        node: w1.clone(),
        tier: OriginTier::Agent,
    };
    hub.local(
        &w1,
        LocalCmd::Forward {
            node: m5.clone(),
            op: "grant.request".to_string(),
            args: json!({"job": "j-1", "names": ["gh"]}).into(),
            origin,
        },
    );
    let timeouts = |hub: &Hub, from: usize| {
        hub.seen[from..]
            .iter()
            .filter(|(n, e)| {
                n == "w1"
                    && matches!(e, Effect::Answer { result: Err(err), .. } if err.code == "timeout")
            })
            .count()
    };
    let start = hub.seen.len();
    for _ in 0..(CALL_MS / PING_MS + 2) {
        hub.tick_all(PING_MS);
    }
    assert_eq!(timeouts(&hub, start), 0);
    for _ in 0..(ATTACH_CALL_MS / PING_MS) {
        hub.tick_all(PING_MS);
    }
    assert_eq!(timeouts(&hub, start), 1);
}

#[test]
fn the_debug_of_a_call_hides_its_arguments() {
    let args = json!({"value": "NL-MARKER-1", "name": "gh"});
    let me = id("m5");
    let origin = Origin {
        node: me.clone(),
        tier: OriginTier::Node,
    };
    let term = Term {
        n: 1,
        node: me.clone(),
    };
    let call = Msg::Call {
        id: 1,
        op: "grant.deliver".to_string(),
        args: args.clone().into(),
        origin: origin.clone(),
        term: term.clone(),
    };
    let forward = LocalCmd::Forward {
        node: id("w1"),
        op: "vault.save".to_string(),
        args: args.clone().into(),
        origin: origin.clone(),
    };
    let run = Effect::RunLocal {
        call: 1,
        op: "grant.deliver".to_string(),
        args: args.into(),
        origin,
        term,
    };
    let shown = [
        format!("{call:?}"),
        format!("{forward:?}"),
        format!("{run:?}"),
        format!(
            "{:?}",
            Effect::Send {
                to: id("w1"),
                msg: call
            }
        ),
        format!(
            "{:?}",
            Input::Local {
                id: 1,
                cmd: forward
            }
        ),
    ];
    for text in shown {
        assert!(!text.contains("NL-MARKER"), "a value shows in Debug");
        assert!(text.contains("hidden"));
    }
}

#[test]
fn a_delegate_job_revokes_its_grants_when_it_ends_either_way() {
    for exit in [0, 3] {
        let (mut hub, m5, w1) = pair();
        let key = format!("d{exit}");
        let spec = JobSpec {
            node: w1.clone(),
            tool: Tool::Agy,
            cwd: "/srv/repo".to_string(),
            args: json!({"delegate": ["agy", "--json", "--", "hi"]}),
            secrets: Vec::new(),
        };
        hub.local(
            &m5,
            LocalCmd::Dispatch {
                key,
                spec,
                tier: OriginTier::App,
            },
        );
        let call = hub
            .seen
            .iter()
            .filter(|(n, _)| n == "w1")
            .flat_map(|(_, e)| run_locals(std::slice::from_ref(e)))
            .find(|(_, op, _)| op == "delegate.run")
            .expect("the run")
            .0;
        let before = hub.seen.len();
        hub.run(
            &w1,
            Input::LocalDone {
                call,
                result: Ok(json!({"exit": exit, "out": "", "err": ""})),
            },
        );
        let revoked = hub.seen[before..]
            .iter()
            .filter(|(n, e)| {
                n == "m5" && matches!(e, Effect::RunLocal { op, .. } if op == GRANT_REVOKE_JOB)
            })
            .count();
        assert_eq!(revoked, 1, "exit {exit}");
    }
}

#[test]
fn a_rollback_revoke_from_a_vault_node_that_stepped_down_is_accepted() {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.join("m5", "w1", "linux", false);
    hub.link(&m1, &w1);
    let newer = Term {
        n: hub.core(&w1).highest_term().expect("term").n + 1,
        node: m1.clone(),
    };
    hub.run(
        &w1,
        Input::Msg {
            from: m1.clone(),
            msg: Msg::Call {
                id: 1,
                op: "node.load".to_string(),
                args: json!({}).into(),
                origin: Origin {
                    node: m1.clone(),
                    tier: OriginTier::Node,
                },
                term: newer.clone(),
            },
        },
    );
    assert_eq!(hub.core(&w1).state().expect("state").home, m1);
    let call = |id: u64, op: &str| Msg::Call {
        id,
        op: op.to_string(),
        args: json!({"job": "j-1"}).into(),
        origin: Origin {
            node: m5.clone(),
            tier: OriginTier::Node,
        },
        term: newer.clone(),
    };
    let revoke = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: call(2, "grant.revoke"),
        },
    );
    assert_eq!(
        run_locals(&revoke)
            .iter()
            .filter(|(_, op, _)| op == "grant.revoke")
            .count(),
        1
    );
    let deliver = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: call(3, "grant.deliver"),
        },
    );
    assert!(run_locals(&deliver).is_empty());
}
