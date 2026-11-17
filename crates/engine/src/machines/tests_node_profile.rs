use super::catalog;
use super::fakes_b::*;
use super::registry::{AdminAccess, Machine, NodeSettings};
use super::step::{CheckState, StepFix, StepId};
use super::tests_node_steps::probe;

fn node_only_linux() -> Machine {
    let mut m = linux("w3");
    m.set_sd_worker(None);
    m.set_admin(AdminAccess::RootSsh);
    m.node = Some(NodeSettings::default());
    m
}

fn node_only_mac() -> Machine {
    let mut m = mac("m3");
    m.set_sd_worker(None);
    m.node = Some(NodeSettings::default());
    m
}

fn m1_like() -> Machine {
    let mut m = mac("m1");
    m.node = Some(NodeSettings::default());
    m
}

fn step_ids(m: &Machine) -> Vec<StepId> {
    catalog::steps(m, std::slice::from_ref(m))
        .into_iter()
        .map(|s| s.id)
        .collect()
}

#[test]
fn node_only_machine_has_no_sd_steps() {
    for m in [node_only_linux(), node_only_mac()] {
        let ids = step_ids(&m);
        for gone in [StepId::BASE_TOOLS, StepId::XCODE] {
            assert!(!ids.contains(&gone), "{} has {}", m.name, gone.raw());
        }
        for kept in [
            StepId::REACHABLE,
            StepId::WORKER_USER,
            StepId::CLAUDE_CODE,
            StepId::TOOL_VERSIONS,
            StepId::NODE_TAILSCALE,
            StepId::NODE_JOINED,
            StepId::NODE_CODEX_LOGIN,
            StepId::CLAUDE_LOGIN,
        ] {
            assert!(ids.contains(&kept), "{} lacks {}", m.name, kept.raw());
        }
        let login = super::tests_node_steps::step(&m, &StepId::CLAUDE_LOGIN);
        let (script, _) = remote(&login);
        assert!(script.contains("cluster status --json"), "{script}");
    }
}

#[test]
fn node_only_machine_has_git_step() {
    for m in [node_only_linux(), node_only_mac()] {
        let ids = step_ids(&m);
        let at = ids.iter().position(|i| *i == StepId::REACHABLE).unwrap();
        assert_eq!(ids[at + 1], StepId::NODE_GIT, "{}", m.name);
        assert!(StepId::NODE_GIT.is_node());
    }
}

#[test]
fn sd_machine_keeps_every_step() {
    let m = m1_like();
    let ids = step_ids(&m);
    for kept in [
        StepId::BASE_TOOLS,
        StepId::XCODE,
        StepId::CLAUDE_LOGIN,
        StepId::NODE_TAILSCALE,
        StepId::NODE_JOINED,
    ] {
        assert!(ids.contains(&kept), "lacks {}", kept.raw());
    }
    assert!(!ids.contains(&StepId::NODE_GIT));
    let mut plain = mac("m4");
    plain.node = None;
    assert!(!step_ids(&plain).contains(&StepId::NODE_GIT));
}

#[test]
fn git_missing_fails() {
    let rig = Rig::new(node_only_linux());
    let on = probe(&rig, &StepId::NODE_GIT, "git version 2.43.0\n", 0);
    assert_eq!(on, CheckState::Pass("git version 2.43.0".to_string()));
    for (text, code) in [
        ("", 1),
        ("", 127),
        ("xcode-select: note: no developer tools were found\n", 1),
    ] {
        let state = probe(&rig, &StepId::NODE_GIT, text, code);
        assert!(state.is_fail(), "{text}: {state:?}");
    }
    let (script, _) = remote(&super::tests_node_steps::step(
        &rig.machine,
        &StepId::NODE_GIT,
    ));
    assert_eq!(script, "command -v git >/dev/null 2>&1 && git --version");
}

#[test]
fn git_fix_is_admin_on_linux() {
    let rig = Rig::new(node_only_linux());
    let step = super::tests_node_steps::step(&rig.machine, &StepId::NODE_GIT);
    let p = plan(&step);
    assert!(p.needs_admin);
    assert!(p.display.contains("apt-get install -y git"));
    assert!(p.display.contains("dnf install -y git"));
    let out = (p.run)(&rig.fix());
    assert!(out.ok, "{}", out.message);
    let calls = rig.argvs();
    assert_eq!(calls.len(), 1);
    let sent = unquoted(last(&calls[0]));
    assert!(sent.contains("apt-get install -y git"), "{sent}");
    assert!(sent.contains("exit 3"), "{sent}");

    let rig = Rig::new(node_only_linux());
    rig.runner.on_script("apt-get", result(3, "", ""));
    let out = (p.run)(&rig.fix());
    assert!(!out.ok);
    assert!(out.message.contains("package manager"), "{}", out.message);
}

#[test]
fn git_on_mac_is_howto() {
    let m = node_only_mac();
    let step = super::tests_node_steps::step(&m, &StepId::NODE_GIT);
    match &step.fix {
        StepFix::HowTo(text) => assert!(text.contains("xcode-select --install"), "{text}"),
        _ => panic!("a Mac's Git is a how-to"),
    }
    assert!(step.plan().is_none());
}
