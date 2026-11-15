use super::fakes_b::*;
use super::registry::Machine;
use super::step::{CheckResult, CheckState, ClusterTokens, RemoteOutput, StepFix, StepId};
use super::tests_node_steps::{node_linux, node_mac, step};
use std::sync::Arc;

const CLUSTER: &str = "cluster-aaaa";
const NODE: &str = "0123456789abcdef0123456789abcdef";
const NOT_NEEDED: &str = "Not needed: the Notchling app runs it";

struct Same;

impl ClusterTokens for Same {
    fn join_token(&self) -> Result<String, String> {
        Ok("nlj1.token".into())
    }
    fn cluster_id(&self) -> Result<Option<String>, String> {
        Ok(Some(CLUSTER.into()))
    }
    fn can_be_master(&self, _machine: &str) -> Option<bool> {
        None
    }
}

fn mac_with_app() -> Machine {
    let mut m = node_mac();
    m.abilities.as_mut().unwrap().notchling_app = Some(true);
    m
}

fn parse(m: &Machine, id: &StepId, text: &str, code: i32) -> CheckResult {
    let rig = Rig::new(m.clone());
    let (_, parse) = remote(&step(m, id));
    let mut ctx = rig.step();
    ctx.tokens = Arc::new(Same);
    parse(
        &RemoteOutput {
            text: text.into(),
            code,
        },
        &ctx,
    )
}

fn script(m: &Machine, id: &StepId) -> String {
    remote(&step(m, id)).0
}

#[test]
fn mac_with_app_skips_binary_and_service() {
    let m = mac_with_app();
    let binary = parse(
        &m,
        &StepId::NODE_BINARY,
        "sh: notchling: not found\nsha256=\napp=yes\n",
        0,
    );
    assert_eq!(binary.state, CheckState::Pass(NOT_NEEDED.into()));
    assert_eq!(binary.detected.notchling_app, Some(true));
    let service = parse(&m, &StepId::NODE_SERVICE, "no daemon is running\n", 3);
    assert_eq!(service.state, CheckState::Pass(NOT_NEEDED.into()));
}

#[test]
fn the_app_is_learned_from_the_binary_probe() {
    let unknown = node_mac();
    let first = parse(
        &unknown,
        &StepId::NODE_BINARY,
        "sh: notchling: not found\nsha256=\napp=yes\n",
        0,
    );
    assert_eq!(first.state, CheckState::Pass(NOT_NEEDED.into()));
    assert_eq!(first.detected.notchling_app, Some(true));

    let without = parse(
        &unknown,
        &StepId::NODE_BINARY,
        "sh: notchling: not found\nsha256=\napp=no\n",
        0,
    );
    assert!(without.state.is_fail(), "{:?}", without.state);
    assert_eq!(without.detected.notchling_app, Some(false));

    let gone = parse(
        &mac_with_app(),
        &StepId::NODE_BINARY,
        "sh: notchling: not found\nsha256=\napp=no\n",
        0,
    );
    assert!(gone.state.is_fail(), "{:?}", gone.state);
    assert_eq!(gone.detected.notchling_app, Some(false));

    let silent = parse(
        &unknown,
        &StepId::NODE_BINARY,
        "sh: notchling: not found\nsha256=\n",
        0,
    );
    assert!(silent.state.is_fail());
    assert_eq!(silent.detected.notchling_app, None);
}

#[test]
fn only_a_mac_asks_whether_the_app_is_there() {
    assert!(script(&node_mac(), &StepId::NODE_BINARY).contains("/Applications/Notchling.app"));
    assert!(!script(&node_linux(), &StepId::NODE_BINARY).contains("Notchling.app"));
    let linux = parse(
        &node_linux(),
        &StepId::NODE_BINARY,
        "sh: notchling: not found\nsha256=\napp=yes\n",
        0,
    );
    assert!(linux.state.is_fail(), "{:?}", linux.state);
    assert_eq!(linux.detected.notchling_app, None);
}

#[test]
fn mac_without_the_app_keeps_every_step_as_before() {
    let m = node_mac();
    let service = parse(&m, &StepId::NODE_SERVICE, "no daemon is running\n", 3);
    assert!(service.state.is_fail(), "{:?}", service.state);
    assert!(script(&m, &StepId::NODE_JOINED).contains(".local/bin"));
    assert!(step(&m, &StepId::NODE_JOINED).plan().is_some());
}

#[test]
fn mac_with_app_join_is_a_howto() {
    let m = mac_with_app();
    let joined = step(&m, &StepId::NODE_JOINED);
    match &joined.fix {
        StepFix::HowTo(text) => assert_eq!(
            text,
            "On m2, open Cluster in Notchling's Chat and join with a token from Copy Join Token here."
        ),
        _ => panic!("a Mac with the app joins from the app, not by a Fix"),
    }
    let probe = script(&m, &StepId::NODE_JOINED);
    assert!(
        probe.contains("/Applications/Notchling.app/Contents/MacOS/notchling"),
        "{probe}"
    );
    assert!(probe.ends_with("cluster status --json"), "{probe}");
    assert!(!probe.contains(".local/bin"), "{probe}");
    let status = format!(
        "{{\"cluster\": {{\"id\": \"{CLUSTER}\", \"name\": \"home\"}}, \"joining\": null, \"me\": \"{NODE}\", \"members\": []}}"
    );
    let ok = parse(&m, &StepId::NODE_JOINED, &status, 0);
    assert!(ok.state.is_pass(), "{:?}", ok.state);
    assert_eq!(ok.detected.node.as_deref(), Some(NODE));
}

#[test]
fn joined_step_says_waiting_while_joining() {
    let m = node_linux();
    let waiting = parse(
        &m,
        &StepId::NODE_JOINED,
        &format!(
            "{{\"cluster\": null, \"joining\": {{\"since\": 5000, \"cluster\": \"{CLUSTER}\", \"primary\": \"{NODE}\", \"answered\": true}}, \"me\": \"{NODE}\", \"members\": []}}"
        ),
        0,
    );
    assert_eq!(
        waiting.state,
        CheckState::CantTell("waiting for approval on this Mac".into())
    );
    let unanswered = parse(
        &m,
        &StepId::NODE_JOINED,
        &format!(
            "{{\"cluster\": null, \"joining\": {{\"since\": 5000, \"answered\": false}}, \"me\": \"{NODE}\", \"members\": []}}"
        ),
        0,
    );
    assert_eq!(
        unanswered.state,
        CheckState::CantTell("joining; this Mac hasn't answered yet".into())
    );
    let nothing = parse(
        &m,
        &StepId::NODE_JOINED,
        &format!("{{\"cluster\": null, \"joining\": null, \"me\": \"{NODE}\", \"members\": []}}"),
        0,
    );
    assert!(nothing.state.is_fail());
    assert_eq!(nothing.state.text(), "not in a cluster yet");
    let old = parse(
        &m,
        &StepId::NODE_JOINED,
        &format!("{{\"cluster\": null, \"me\": \"{NODE}\", \"members\": []}}"),
        0,
    );
    assert!(old.state.is_fail());
}

#[test]
fn old_unit_without_killmode_fails_service_check() {
    let m = node_linux();
    let old = parse(
        &m,
        &StepId::NODE_SERVICE,
        "active=active\nkillmode=KillMode=control-group\nlinger=Linger=yes\n",
        0,
    );
    assert_eq!(
        old.state,
        CheckState::Fail("the service file is out of date".into())
    );
    let fresh = parse(
        &m,
        &StepId::NODE_SERVICE,
        "active=active\nkillmode=KillMode=process\nlinger=Linger=yes\n",
        0,
    );
    assert!(fresh.state.is_pass(), "{:?}", fresh.state);
    let unsaid = parse(
        &m,
        &StepId::NODE_SERVICE,
        "active=active\nlinger=Linger=yes\n",
        0,
    );
    assert!(
        matches!(unsaid.state, CheckState::CantTell(_)),
        "{:?}",
        unsaid.state
    );
    let down = parse(
        &m,
        &StepId::NODE_SERVICE,
        "active=inactive\nkillmode=KillMode=control-group\nlinger=Linger=yes\n",
        0,
    );
    assert_eq!(
        down.state,
        CheckState::Fail("the service is inactive".into())
    );
    let probe = script(&m, &StepId::NODE_SERVICE);
    assert!(
        probe.contains("systemctl --user show notchlingd.service -p KillMode"),
        "{probe}"
    );
}

#[test]
fn the_restart_warning_is_true_for_each_system() {
    let linux = plan(&step(&node_linux(), &StepId::NODE_SERVICE)).display;
    assert!(
        !linux.contains("Work running on this machine stops"),
        "{linux}"
    );
    assert!(linux.contains("restarted service doesn't know"), "{linux}");
    let linux_binary = plan(&step(&node_linux(), &StepId::NODE_BINARY)).display;
    assert!(linux_binary.contains("restarted service doesn't know"));
    let mac = plan(&step(&node_mac(), &StepId::NODE_SERVICE)).display;
    assert!(
        mac.starts_with("Work running on this machine stops while the service restarts."),
        "{mac}"
    );
}
