use super::catalog;
use super::fakes_b::*;
use super::step::{CheckState, RemoteOutput, StepFix, StepId};
use super::steps_secret::{cluster_how_to, cluster_login_result, SENT_BY_THE_CLUSTER};
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

fn result(rig: &Rig, logins: &str, on: bool) -> CheckState {
    let out = RemoteOutput {
        text: status(logins),
        code: 0,
    };
    cluster_login_result(&out, &rig.step(), on).state
}

#[test]
fn with_the_cluster_login_on_the_step_fails_until_the_node_reports_a_standing_login() {
    let rig = Rig::new(node_only());
    for logins in [r#"{"claude":false}"#, r#"{}"#, r#"{"codex":true}"#] {
        let state = result(&rig, logins, true);
        assert!(state.is_fail(), "{logins}: {state:?}");
    }
    assert!(matches!(
        result(&rig, r#"{"claude":true}"#, true),
        CheckState::Pass(_)
    ));
}

#[test]
fn while_the_cluster_login_is_off_a_missing_login_cannot_be_told_and_is_no_failure() {
    const { assert!(!crate::cluster::login_files::CLUSTER_LOGIN_ON) };
    let rig = Rig::new(node_only());
    for logins in [r#"{"claude":false}"#, r#"{}"#] {
        let state = result(&rig, logins, false);
        assert!(matches!(state, CheckState::CantTell(_)), "{state:?}");
        let real = probe(&rig, &StepId::CLAUDE_LOGIN, &status(logins), 0);
        assert!(matches!(real, CheckState::CantTell(_)), "{real:?}");
    }
    assert!(matches!(
        result(&rig, r#"{"claude":true}"#, false),
        CheckState::Pass(_)
    ));
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
                text.contains("setup-token") && text.contains("isn't available in this build"),
                "{text}"
            );
            assert!(!text.contains("claude-worker"), "{text}");
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

#[test]
fn a_node_only_worker_has_no_claude_env_usage_and_an_sd_worker_node_keeps_it() {
    let only = node_only();
    assert!(super::steps_secret::usages(std::slice::from_ref(&only))
        .iter()
        .all(|u| u.remote_path != super::shell::CLAUDE_ENV));
    let both = node_linux();
    assert!(super::steps_secret::usages(std::slice::from_ref(&both))
        .iter()
        .any(|u| u.remote_path == super::shell::CLAUDE_ENV));
}

#[test]
fn with_the_login_on_the_how_to_names_the_cluster_entry_the_allowlist_and_a_differing_secret() {
    let mut m = node_only();
    m.claude_secret = "my-own-claude".to_string();
    let text = cluster_how_to(&m, true);
    assert!(
        text.contains("claude-worker") && text.contains("expiry"),
        "{text}"
    );
    assert!(text.contains("allow it on this machine"), "{text}");
    assert!(
        text.contains("my-own-claude") && text.contains("only"),
        "{text}"
    );
    let same = node_only();
    assert!(!cluster_how_to(&same, true).contains("isn't used"));
}

#[test]
fn while_the_login_is_off_the_how_to_says_to_sign_in_there_by_hand() {
    let mut m = node_only();
    m.claude_secret = "my-own-claude".to_string();
    let text = cluster_how_to(&m, false);
    assert!(text.contains("isn't available in this build yet"), "{text}");
    assert!(
        text.contains("yourself") && text.contains("setup-token"),
        "{text}"
    );
    assert!(
        !text.contains("claude-worker") && !text.contains("my-own-claude"),
        "{text}"
    );
}
