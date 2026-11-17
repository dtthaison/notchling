use super::fakes_b::*;
use super::registry::{AdminAccess, Machine};
use super::step::{CheckState, ClusterTokens, RemoteOutput, StepId};
use super::steps_node_mac::data_folder_script;
use super::steps_node_service::log_folder_script;
use super::tests_node_steps::{fix_of, node_linux, node_mac, probe, step, texts};
use std::sync::Arc;

fn with_arch(mut m: Machine, arch: &str) -> Machine {
    m.abilities.as_mut().unwrap().arch = Some(arch.into());
    m
}

fn dist_with(m: &Machine, name: &str) -> TempDir {
    let dist = TempDir::new("final-dist");
    std::fs::write(dist.path().join(name), b"build").unwrap();
    let _ = m;
    dist
}

#[test]
fn a_hostile_arch_runs_nothing_and_the_fix_fails() {
    for arch in ["x86_64/../../x", "a b", "", "x86;rm", "X86_64"] {
        let rig = Rig::new(with_arch(node_linux(), arch));
        let dist = dist_with(&rig.machine, "notchling-linux-x86_64");
        let ctx = fix_of(&rig, Some(dist.path()), None);
        let out = (plan(&step(&rig.machine, &StepId::NODE_BINARY)).run)(&ctx);
        assert!(!out.ok, "{arch:?}: {}", out.message);
        assert!(
            rig.calls().is_empty(),
            "{arch:?}: {:?}",
            texts(&rig.calls())
        );
    }
}

#[test]
fn a_linux_machine_set_to_sudo_is_refused_before_any_command() {
    let mut m = node_linux();
    m.set_admin(AdminAccess::Sudo);
    let rig = Rig::new(m);
    let ctx = fix_of(&rig, None, None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_SERVICE)).run)(&ctx);
    assert!(!out.ok, "{}", out.message);
    assert!(rig.calls().is_empty(), "{:?}", texts(&rig.calls()));
}

#[test]
fn a_user_or_host_that_breaks_the_rules_reaches_no_scp_and_no_ssh() {
    for (host, user) in [
        ("-oProxyCommand=x", "agent"),
        ("w1.test", "a b"),
        ("w1.test", "-oBad"),
        ("w1 test", "agent"),
        ("w1.test;x", "agent"),
    ] {
        let mut m = node_linux();
        m.host = host.into();
        m.user = user.into();
        let rig = Rig::new(m);
        let dist = dist_with(&rig.machine, "notchling-linux-x86_64");
        let ctx = fix_of(&rig, Some(dist.path()), Some("cluster-aaaa"));
        for id in [
            StepId::NODE_BINARY,
            StepId::NODE_SERVICE,
            StepId::NODE_JOINED,
        ] {
            let out = (plan(&step(&rig.machine, &id)).run)(&ctx);
            assert!(!out.ok, "{host:?} {user:?} {}", id.raw());
            assert!(out.message.contains("won't run on w1"), "{}", out.message);
        }
        assert!(
            rig.calls().is_empty(),
            "{host:?} {user:?}: {:?}",
            texts(&rig.calls())
        );
    }
}

struct Down;

impl ClusterTokens for Down {
    fn join_token(&self) -> Result<String, String> {
        panic!("no token is asked for by a check")
    }
    fn cluster_id(&self) -> Result<Option<String>, String> {
        Err("the daemon didn't answer".into())
    }
    fn can_be_master(&self, _machine: &str) -> Option<bool> {
        None
    }
}

#[test]
fn a_daemon_that_is_down_reads_as_cant_tell_not_as_no_cluster() {
    let rig = Rig::new(node_linux());
    let (_, parse) = remote(&step(&rig.machine, &StepId::NODE_JOINED));
    let mut ctx = rig.step();
    ctx.tokens = Arc::new(Down);
    let result = parse(
        &RemoteOutput {
            text: "{\"cluster\":null}".into(),
            code: 0,
        },
        &ctx,
    );
    assert_eq!(
        result.state,
        CheckState::CantTell("Notchling isn't running on this Mac".into())
    );
}

#[test]
fn the_log_folder_is_made_before_the_service_file_on_a_fresh_machine() {
    for linux in [true, false] {
        let rig = Rig::new(if linux { node_linux() } else { node_mac() });
        let ctx = fix_of(&rig, None, None);
        let out = (plan(&step(&rig.machine, &StepId::NODE_SERVICE)).run)(&ctx);
        assert!(out.ok, "{}", out.message);
        let seen = texts(&rig.calls());
        assert!(seen[0].contains("mkdir -p"), "{seen:?}");
        if linux {
            assert!(seen[1].contains("service install"), "{seen:?}");
        } else {
            assert!(seen[0].contains("notchlingd.log"), "{seen:?}");
            assert!(seen[1].contains("launchctl bootout"), "{seen:?}");
            assert!(
                seen[2].contains("install -m 644 -o root -g wheel"),
                "{seen:?}"
            );
        }
    }
}

fn made_by(script: &str, home: &std::path::Path, xdg: Option<&str>) {
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.arg("-c").arg(script).env_clear().env("HOME", home);
    if let Some(x) = xdg {
        cmd.env("XDG_DATA_HOME", x);
    }
    let out = cmd.output().expect("sh runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn the_log_folder_script_makes_the_folder_the_daemon_writes_to() {
    let home = TempDir::new("final-home");
    made_by(&log_folder_script(), home.path(), None);
    assert!(home.path().join(".local/share/notchling").is_dir());
    let other = TempDir::new("final-xdg");
    let xdg = other.path().join("data");
    made_by(
        &log_folder_script(),
        home.path(),
        Some(&xdg.to_string_lossy()),
    );
    assert!(xdg.join("notchling").is_dir());
    let relative = TempDir::new("final-rel-home");
    made_by(&log_folder_script(), relative.path(), Some("relative/x"));
    assert!(relative.path().join(".local/share/notchling").is_dir());
    assert!(!relative.path().join("relative").exists());
    let mac = TempDir::new("final-mac-home");
    made_by(data_folder_script(), mac.path(), None);
    assert!(mac
        .path()
        .join("Library/Application Support/Notchling/notchlingd.log")
        .is_file());
}

#[test]
fn a_binary_that_is_there_but_does_not_run_says_so() {
    let rig = Rig::new(node_linux());
    let text = "sh: 1: notchling: Exec format error\nsha256=abc123\n";
    let state = probe(&rig, &StepId::NODE_BINARY, text, 0);
    assert!(state.text().contains("but doesn't run"), "{state:?}");
    let missing = probe(
        &rig,
        &StepId::NODE_BINARY,
        "sh: notchling: not found\nsha256=\n",
        127,
    );
    assert!(missing.text().contains("isn't installed"), "{missing:?}");
}

#[test]
fn tailscale_missing_on_linux_names_no_mac_path() {
    let rig = Rig::new(node_linux());
    let state = probe(
        &rig,
        &StepId::NODE_TAILSCALE,
        "sh: tailscale: not found",
        127,
    );
    assert_eq!(state.text(), "Tailscale isn't installed there");
    let (script, _) = remote(&step(&rig.machine, &StepId::NODE_TAILSCALE));
    assert!(!script.contains("/Applications"), "{script}");
    let mac = Rig::new(node_mac());
    let (script, _) = remote(&step(&mac.machine, &StepId::NODE_TAILSCALE));
    assert!(script.contains("/Applications/Tailscale.app"), "{script}");
}

#[test]
fn a_service_answer_that_is_not_a_state_has_its_own_wording() {
    let rig = Rig::new(node_linux());
    let said = "active=Failed to connect to bus: No medium found\nlinger=Linger=yes\n";
    let state = probe(&rig, &StepId::NODE_SERVICE, said, 0);
    assert!(
        state.text().contains("service manager didn't answer"),
        "{state:?}"
    );
    let plain = probe(
        &rig,
        &StepId::NODE_SERVICE,
        "active=failed\nlinger=Linger=yes\n",
        0,
    );
    assert_eq!(plain.text(), "the service is failed");
}

#[test]
fn a_failed_copy_removes_the_partial_file_and_the_sheet_shows_every_command() {
    let rig = Rig::new(node_linux());
    rig.runner.on(
        |a| a.first().is_some_and(|p| p == "/usr/bin/scp"),
        super::runner::CommandResult::new(1, "", "connection lost"),
    );
    let dist = dist_with(&rig.machine, "notchling-linux-x86_64");
    let ctx = fix_of(&rig, Some(dist.path()), None);
    let binary = step(&rig.machine, &StepId::NODE_BINARY);
    let out = (plan(&binary).run)(&ctx);
    assert!(!out.ok);
    let seen = texts(&rig.calls());
    assert!(
        seen.last().unwrap().contains("rm -f"),
        "the partial copy is removed: {seen:?}"
    );
    let shown = plan(&binary).display;
    for part in [
        "/usr/bin/scp -O -q -o BatchMode=yes -o ConnectTimeout=8 --",
        "rm -f",
        "chmod 755",
        "XDG_RUNTIME_DIR",
        "restarted service doesn't know",
    ] {
        assert!(shown.contains(part), "{part}: {shown}");
    }
    let service = plan(&step(&rig.machine, &StepId::NODE_SERVICE)).display;
    for part in [
        "mkdir -p",
        "XDG_RUNTIME_DIR",
        "bus",
        "restarted service doesn't know",
    ] {
        assert!(service.contains(part), "{part}: {service}");
    }
}

#[test]
fn the_move_is_chmod_first_so_a_half_made_file_is_never_in_place() {
    let rig = Rig::new(node_linux());
    let dist = dist_with(&rig.machine, "notchling-linux-x86_64");
    let digest =
        super::steps_node_binary::file_hash(&dist.path().join("notchling-linux-x86_64")).unwrap();
    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("sha256sum")),
        super::runner::CommandResult::ok(&format!("{digest}  notchling.new\n")),
    );
    let ctx = fix_of(&rig, Some(dist.path()), None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_BINARY)).run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let seen = texts(&rig.calls());
    let moved = seen.iter().find(|t| t.contains("mv -f")).unwrap();
    assert!(
        moved.find("chmod 755").unwrap() < moved.find("mv -f").unwrap(),
        "{moved}"
    );
}
