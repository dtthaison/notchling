use super::catalog;
use super::fakes::TestMachines;
use super::fakes_b::*;
use super::step::{CheckState, ClusterTokens, Probe, SetupStep, StepFix, StepId};
use super::steps_master::only_masters;
use super::tests_node_steps::{node_linux, node_mac};
use crate::cluster::ready::{CheckKind, Checks};
use std::sync::Arc;

const MASTERS: [StepId; 4] = [
    StepId::MASTER_ENCRYPTED,
    StepId::MASTER_SAME_HOME,
    StepId::MASTER_SPACE,
    StepId::MASTER_TOOLS,
];

struct Facts {
    checks: Option<Checks>,
}

impl ClusterTokens for Facts {
    fn join_token(&self) -> Result<String, String> {
        panic!("a readiness check never asks for the join token")
    }
    fn cluster_id(&self) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn can_be_master(&self, _machine: &str) -> Option<bool> {
        None
    }
    fn master_checks(&self, node: Option<&str>) -> Option<Checks> {
        node?;
        self.checks.clone()
    }
}

fn good() -> Checks {
    vec![
        (
            CheckKind::Encrypted,
            Ok("disk encryption is on".to_string()),
        ),
        (CheckKind::SameHome, Ok("son at /Users/son".to_string())),
        (CheckKind::Space, Ok("80.0 GB free".to_string())),
        (CheckKind::Tools, Ok("Claude Code 2.1.3".to_string())),
    ]
}

fn unencrypted() -> Checks {
    let mut list = good();
    list[0].1 = Err("m2 has no disk encryption".to_string());
    list
}

fn step_of(m: &super::registry::Machine, id: &StepId) -> SetupStep {
    catalog::steps(m, std::slice::from_ref(m))
        .into_iter()
        .find(|s| &s.id == id)
        .unwrap_or_else(|| panic!("no step {}", id.raw()))
}

fn state(id: &StepId, checks: Option<Checks>, node: Option<&str>) -> CheckState {
    let rig = Rig::new(node_mac());
    let mut ctx = rig.step();
    ctx.tokens = Arc::new(Facts { checks });
    ctx.node = node.map(str::to_string);
    let Probe::Local(probe) = step_of(&rig.machine, id).probe else {
        panic!("a master step reads the cluster, it has no remote probe");
    };
    probe(&ctx).state
}

#[test]
fn every_master_step_passes_when_the_cluster_reports_it_ready() {
    for id in &MASTERS {
        let got = state(id, Some(good()), Some("n1"));
        assert!(got.is_pass(), "{} {got:?}", id.raw());
    }
}

#[test]
fn unencrypted_fails_with_howto() {
    let got = state(&StepId::MASTER_ENCRYPTED, Some(unencrypted()), Some("n1"));
    assert_eq!(
        got,
        CheckState::Fail("m2 has no disk encryption".to_string())
    );
    let mac = node_mac();
    match step_of(&mac, &StepId::MASTER_ENCRYPTED).fix {
        StepFix::HowTo(text) => assert!(text.contains("FileVault"), "{text}"),
        _ => panic!("the encryption step needs a how-to"),
    }
    let linux = node_linux();
    match step_of(&linux, &StepId::MASTER_ENCRYPTED).fix {
        StepFix::HowTo(text) => assert!(text.contains("LUKS"), "{text}"),
        _ => panic!("the encryption step needs a how-to"),
    }
}

#[test]
fn only_the_failing_check_fails() {
    let ok = state(&StepId::MASTER_SPACE, Some(unencrypted()), Some("n1"));
    assert!(ok.is_pass(), "{ok:?}");
}

#[test]
fn a_member_the_cluster_hasnt_reported_is_cant_tell() {
    assert!(matches!(
        state(&StepId::MASTER_TOOLS, None, Some("n1")),
        CheckState::CantTell(_)
    ));
    assert!(matches!(
        state(&StepId::MASTER_TOOLS, Some(good()), None),
        CheckState::CantTell(_)
    ));
}

#[test]
fn every_master_step_has_a_howto_and_waits_for_the_join() {
    for machine in [node_mac(), node_linux()] {
        for id in &MASTERS {
            let step = step_of(&machine, id);
            assert!(matches!(step.fix, StepFix::HowTo(_)), "{}", id.raw());
            assert_eq!(step.needs, vec![StepId::NODE_JOINED]);
        }
    }
}

#[test]
fn master_steps_exist_only_for_a_machine_that_runs_a_node() {
    let plain = TestMachines::mac("m3");
    let ids: Vec<StepId> = catalog::steps(&plain, std::slice::from_ref(&plain))
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert!(MASTERS.iter().all(|id| !ids.contains(id)));
    let node = node_mac();
    let ids: Vec<StepId> = catalog::steps(&node, std::slice::from_ref(&node))
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert!(MASTERS.iter().all(|id| ids.contains(id)));
}

#[test]
fn steps_are_shown_only_when_the_member_can_be_master() {
    let machine = node_mac();
    let all = catalog::steps(&machine, std::slice::from_ref(&machine));
    let on = Facts {
        checks: Some(good()),
    };
    let shown = only_masters(all.clone(), &on, Some("n1"));
    assert!(MASTERS.iter().all(|id| shown.iter().any(|s| &s.id == id)));
    let off = Facts { checks: None };
    let hidden = only_masters(all.clone(), &off, Some("n1"));
    assert!(MASTERS.iter().all(|id| !hidden.iter().any(|s| &s.id == id)));
    assert_eq!(hidden.len(), all.len() - MASTERS.len());
}
