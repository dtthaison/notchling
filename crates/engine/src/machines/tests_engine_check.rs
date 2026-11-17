use super::catalog;
use super::fakes_b::TempDir;
use super::registry::{AdminAccess, Machine, MachineAbilities, MachineOs, NodeSettings};
use super::runner::{Call, CommandResult};
use super::step::{CheckState, NoCluster, StepId};
use super::tests_engine_world::{batch_probes, Setup, World};
use super::tests_node_steps::hex;
use std::sync::Arc;

const BUILD: &[u8] = b"the mac's build";

const REACHABLE: &str = "os=Linux\narch=x86_64\nuid=1000\nuser=agent\nhome=/home/agent\nversion=Ubuntu 24.04\ndisk_kb=52428800\ndocker=none";

fn new_linux_node() -> Machine {
    let mut m = Machine::new("w1", "w1.test", "agent");
    m.set_admin(AdminAccess::RootSsh);
    m.node = Some(NodeSettings::default());
    m
}

fn answer(call: &Call) -> CommandResult {
    let script = call.argv.last().cloned().unwrap_or_default();
    let Some((nonce, probes)) = batch_probes(&script) else {
        return CommandResult::ok("");
    };
    let installed = format!("notchling 0.0.9\nsha256={}", hex(BUILD));
    let mut out = String::new();
    for (i, probe) in probes.iter().enumerate() {
        let text = if probe.contains("uname -m") {
            REACHABLE.to_string()
        } else if probe.contains("--version") && probe.contains("sha256=") {
            installed.clone()
        } else {
            "ok".to_string()
        };
        out.push_str(&format!("{text}\n@@{nonce} {i} 0@@\n"));
    }
    CommandResult::ok(&out)
}

fn world_of(dist: &TempDir, machine: Machine) -> World {
    std::fs::write(dist.path().join("notchling-linux-x86_64"), BUILD).unwrap();
    let mut setup = Setup::new(catalog::steps);
    setup.machines = vec![machine];
    setup.runner.on_call(
        |argv| {
            argv.last()
                .is_some_and(|s| s.contains("out=$( /bin/sh -c "))
        },
        answer,
    );
    let mut w = setup.build();
    w.engine = w
        .engine
        .with_cluster(Arc::new(NoCluster), Some(dist.path().to_path_buf()));
    w
}

fn world(dist: &TempDir) -> World {
    world_of(dist, new_linux_node())
}

#[test]
fn the_first_refresh_of_a_new_linux_node_with_a_binary_does_not_stop_at_the_chip() {
    let dist = TempDir::new("check-chip-refresh");
    let w = world(&dist);
    w.refresh("w1", false);
    let binary = w.state("w1", "node-binary");
    assert!(
        matches!(binary, Some(CheckState::Pass(_))),
        "node-binary: {binary:?}"
    );
    let service = w.state("w1", "node-service");
    assert_ne!(
        service,
        Some(CheckState::CantTell("waits for Notchling installed".into()))
    );
}

#[test]
fn the_first_check_of_a_draft_linux_node_with_a_binary_does_not_stop_at_the_chip() {
    let dist = TempDir::new("check-chip-draft");
    let w = world(&dist);
    let results = w.engine.check_draft(&new_linux_node());
    let binary = results.get(&StepId::NODE_BINARY).map(|r| r.state.clone());
    assert!(
        matches!(binary, Some(CheckState::Pass(_))),
        "node-binary: {binary:?}"
    );
    let service = results.get(&StepId::NODE_SERVICE).map(|r| r.state.clone());
    assert_ne!(
        service,
        Some(CheckState::CantTell("waits for Notchling installed".into()))
    );
}

#[test]
fn a_refresh_of_a_machine_whose_probe_facts_are_known_looks_once_even_when_other_abilities_changed()
{
    let dist = TempDir::new("check-chip-once");
    let mut known = new_linux_node();
    known.os = Some(MachineOs::Linux);
    known.abilities = Some(MachineAbilities {
        home: Some("/home/agent".into()),
        arch: Some("x86_64".into()),
        free_disk_gb: Some(1),
        os_version: Some("Ubuntu 22.04".into()),
        ..MachineAbilities::default()
    });
    let w = world_of(&dist, known);
    w.refresh("w1", false);
    assert_eq!(w.batches().len(), 1);
    let binary = w.state("w1", "node-binary");
    assert!(
        matches!(binary, Some(CheckState::Pass(_))),
        "node-binary: {binary:?}"
    );
}
