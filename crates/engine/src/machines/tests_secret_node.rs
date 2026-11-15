use super::catalog;
use super::fakes_b::*;
use super::step::{CheckState, StepFix, StepId};
use super::steps_secret::SENT_BY_THE_CLUSTER;
use super::tests_node_steps::{node_linux, probe, step};

const ME: &str = "0123456789abcdef0123456789abcdef";

fn node_only() -> super::registry::Machine {
    let mut m = node_linux();
    m.set_sd_worker(None);
    m
}

fn status(logins: &str) -> String {
    format!(
        r#"{{"cluster":{{"id":"c"}},"me":"{ME}","members":[{{"id":"other","versions":null}},{{"id":"{ME}","versions":{{"notchling":"1","logins":{logins}}}}}]}}"#
    )
}

#[test]
fn a_node_without_an_sd_worker_says_sent_by_the_cluster_when_it_holds_the_login() {
    let rig = Rig::new(node_only());
    let state = probe(
        &rig,
        &StepId::CLAUDE_LOGIN,
        &status(r#"{"claude":true}"#),
        0,
    );
    assert_eq!(state, CheckState::Pass(SENT_BY_THE_CLUSTER.to_string()));
    assert_eq!(SENT_BY_THE_CLUSTER, "Sent by the cluster");
}

#[test]
fn the_step_fails_until_the_node_reports_a_standing_login() {
    let rig = Rig::new(node_only());
    for logins in [r#"{"claude":false}"#, r#"{}"#, r#"{"codex":true}"#] {
        let state = probe(&rig, &StepId::CLAUDE_LOGIN, &status(logins), 0);
        assert!(state.is_fail(), "{logins}: {state:?}");
    }
}

#[test]
fn an_unreadable_answer_or_an_unreported_node_cannot_be_told() {
    let rig = Rig::new(node_only());
    let silent = r#"{"me":"abc","members":[{"id":"abc","versions":null}]}"#;
    for text in ["sh: notchling: not found", "{}", silent] {
        let state = probe(&rig, &StepId::CLAUDE_LOGIN, text, 0);
        assert!(
            matches!(state, CheckState::CantTell(_)),
            "{text}: {state:?}"
        );
    }
}

#[test]
fn the_step_reads_the_nodes_own_status_and_has_no_fix_that_writes() {
    let m = node_only();
    let found = step(&m, &StepId::CLAUDE_LOGIN);
    let (script, _) = remote(&found);
    assert!(script.contains("cluster status --json"));
    assert!(!script.contains("claude.env"));
    match &found.fix {
        StepFix::HowTo(text) => {
            assert!(
                text.contains("claude-worker") && text.contains("expiry"),
                "{text}"
            );
        }
        _ => panic!("the cluster sends the login; the step has a how-to, not a Fix"),
    }
    assert!(found.needs.contains(&StepId::NODE_JOINED));
}

#[test]
fn a_machine_that_is_not_a_node_keeps_the_claude_env_step() {
    let mut m = node_only();
    m.node = None;
    let (script, _) = remote(&find(
        &catalog::steps(&m, std::slice::from_ref(&m)),
        &StepId::CLAUDE_LOGIN,
    ));
    assert!(script.contains("claude.env"));
}

#[test]
fn a_node_that_is_also_an_sd_worker_keeps_the_claude_env_step() {
    let m = node_linux();
    assert!(m.node.is_some() && m.sd_worker().is_some());
    let (script, _) = remote(&step(&m, &StepId::CLAUDE_LOGIN));
    assert!(script.contains("claude.env"));
}
