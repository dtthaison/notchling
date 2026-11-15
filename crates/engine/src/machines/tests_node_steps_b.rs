use super::fakes_b::*;
use super::runner::CommandResult;
use super::step::{CheckState, RemoteOutput, StepId};
use super::steps_node::{load_command, restart_command, MAC_BUILD};
use super::tests_node_steps::{fix_of, hex, node_linux, node_mac, step, texts};

const BUILD: &[u8] = b"the mac's build";

fn binary_probe(rig: &Rig, dist: Option<&std::path::Path>, text: &str) -> CheckState {
    let (_, parse) = remote(&step(&rig.machine, &StepId::NODE_BINARY));
    let mut ctx = rig.step();
    ctx.dist_dir = dist.map(std::path::Path::to_path_buf);
    parse(
        &RemoteOutput {
            text: text.into(),
            code: 0,
        },
        &ctx,
    )
    .state
}

#[test]
fn binary_probe_prints_the_installed_hash() {
    let rig = Rig::new(node_linux());
    let (script, _) = remote(&step(&rig.machine, &StepId::NODE_BINARY));
    assert!(script.contains("--version"));
    assert!(script.contains("sha256sum"));
    assert!(script.contains("shasum -a 256"));
    assert!(script.contains("sha256="));
}

#[test]
fn binary_with_other_hash_fails_when_dist_differs() {
    let rig = Rig::new(node_linux());
    let dist = TempDir::new("dist-hash");
    std::fs::write(dist.path().join("notchling-linux-x86_64"), BUILD).unwrap();
    let old = format!("{MAC_BUILD}\nsha256={}\n", hex(b"an older build"));
    let state = binary_probe(&rig, Some(dist.path()), &old);
    assert!(state.is_fail(), "{state:?}");
}

#[test]
fn binary_with_matching_hash_passes() {
    let rig = Rig::new(node_linux());
    let dist = TempDir::new("dist-same");
    std::fs::write(dist.path().join("notchling-linux-x86_64"), BUILD).unwrap();
    let same = format!("notchling 0.0.9\nsha256={}\n", hex(BUILD));
    let state = binary_probe(&rig, Some(dist.path()), &same);
    assert!(state.is_pass(), "{state:?}");
}

#[test]
fn binary_without_a_dist_file_falls_back_to_the_version() {
    let rig = Rig::new(node_linux());
    let dist = TempDir::new("dist-none");
    let text = format!("{MAC_BUILD}\nsha256={}\n", hex(b"anything"));
    assert!(binary_probe(&rig, Some(dist.path()), &text).is_pass());
    assert!(binary_probe(&rig, None, &text).is_pass());
    let other = format!("notchling 0.0.1\nsha256={}\n", hex(b"anything"));
    assert!(binary_probe(&rig, Some(dist.path()), &other).is_fail());
    assert!(binary_probe(&rig, None, "sh: notchling: No such file\nsha256=\n").is_fail());
}

#[test]
fn a_replaced_dist_file_is_hashed_again() {
    let rig = Rig::new(node_linux());
    let dist = TempDir::new("dist-swap");
    let file = dist.path().join("notchling-linux-x86_64");
    std::fs::write(&file, BUILD).unwrap();
    let first = format!("{MAC_BUILD}\nsha256={}\n", hex(BUILD));
    assert!(binary_probe(&rig, Some(dist.path()), &first).is_pass());
    std::fs::write(&file, b"a different and longer build").unwrap();
    assert!(binary_probe(&rig, Some(dist.path()), &first).is_fail());
}

#[test]
fn binary_fix_restarts_the_service_after_the_move() {
    let rig = Rig::new(node_linux());
    let dist = TempDir::new("dist-restart");
    std::fs::write(dist.path().join("notchling-linux-x86_64"), BUILD).unwrap();
    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("sha256sum")),
        ok(&format!("{}  new\n", hex(BUILD))),
    );
    let ctx = fix_of(&rig, Some(dist.path()), None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_BINARY)).run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let seen = texts(&rig.calls());
    let moved = seen.iter().position(|t| t.contains("mv -f")).unwrap();
    let restart = seen
        .iter()
        .position(|t| t.contains("is-active --quiet notchlingd.service"))
        .expect("a restart");
    assert!(moved < restart, "{seen:?}");
    assert!(seen[restart].contains("systemctl --user restart notchlingd.service"));

    let mac = Rig::new(node_mac());
    let dist = TempDir::new("dist-restart-mac");
    std::fs::write(dist.path().join("notchling-macos-aarch64"), BUILD).unwrap();
    mac.runner.on(
        |a| a.last().is_some_and(|s| s.contains("shasum -a 256")),
        ok(&format!("{}  new\n", hex(BUILD))),
    );
    let ctx = fix_of(&mac, Some(dist.path()), None);
    let out = (plan(&step(&mac.machine, &StepId::NODE_BINARY)).run)(&ctx);
    assert!(out.ok, "{}", out.message);
    let seen = texts(&mac.calls());
    let last = seen.last().unwrap();
    assert!(last.contains("launchctl print"), "{last}");
    assert!(last.contains("launchctl kickstart -k"));
    assert!(
        !last.contains("bootstrap"),
        "a restart never loads the service"
    );
}

#[test]
fn a_restart_that_fails_fails_the_binary_fix() {
    let rig = Rig::new(node_mac());
    let dist = TempDir::new("dist-restart-fail");
    std::fs::write(dist.path().join("notchling-macos-aarch64"), BUILD).unwrap();
    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("kickstart")),
        CommandResult::new(1, "", "kickstart failed"),
    );
    rig.runner.on(
        |a| a.last().is_some_and(|s| s.contains("shasum -a 256")),
        ok(&format!("{}  new\n", hex(BUILD))),
    );
    let ctx = fix_of(&rig, Some(dist.path()), None);
    let out = (plan(&step(&rig.machine, &StepId::NODE_BINARY)).run)(&ctx);
    assert!(!out.ok);
    assert!(out.message.contains("didn't restart"), "{}", out.message);
}

#[test]
fn scp_brackets_an_ipv6_host() {
    let mut m = node_linux();
    m.host = "fd7a:115c:a1e0::1".into();
    let rig = Rig::new(m);
    let dist = TempDir::new("dist-v6");
    std::fs::write(dist.path().join("notchling-linux-x86_64"), BUILD).unwrap();
    let ctx = fix_of(&rig, Some(dist.path()), None);
    let _ = (plan(&step(&rig.machine, &StepId::NODE_BINARY)).run)(&ctx);
    let calls = rig.calls();
    let scp = calls
        .iter()
        .find(|c| c.argv.first().is_some_and(|a| a == "/usr/bin/scp"))
        .expect("an scp call");
    assert_eq!(
        scp.argv.last().unwrap(),
        "agent@[fd7a:115c:a1e0::1]:.local/bin/notchling.new"
    );
    let shown = plan(&step(&rig.machine, &StepId::NODE_BINARY)).display;
    assert!(shown.contains("agent@[fd7a:115c:a1e0::1]:.local/bin/notchling.new"));
}

#[test]
fn fix_displays_name_the_machine() {
    let rig = Rig::new(node_linux());
    let binary = plan(&step(&rig.machine, &StepId::NODE_BINARY)).display;
    assert!(binary.contains("scp -O notchling-linux-x86_64 agent@w1.test:"));
    assert!(!binary.contains('<'), "{binary}");
    let service = plan(&step(&rig.machine, &StepId::NODE_SERVICE)).display;
    assert!(service.contains("loginctl enable-linger 'agent'"));
    assert!(!service.contains("<user>"));
    let again = plan(&step(&rig.machine, &StepId::NODE_BINARY)).display;
    assert_eq!(binary, again);

    let mut unknown = node_mac();
    unknown.abilities.as_mut().unwrap().arch = None;
    let shown = plan(&step(&unknown, &StepId::NODE_BINARY)).display;
    assert!(shown.contains("notchling-macos-<arch>"), "{shown}");
}

#[test]
fn linux_load_enables_then_restarts() {
    assert_eq!(
        load_command(true),
        "systemctl --user daemon-reload && systemctl --user enable notchlingd.service && systemctl --user restart notchlingd.service"
    );
    assert!(restart_command(true).contains("is-active --quiet"));
}

#[cfg(unix)]
mod launchctl {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    fn run(script: &str, loaded: bool) -> Vec<String> {
        let dir = TempDir::new("launchctl-stub");
        let log = dir.path().join("log");
        let stub = dir.path().join("launchctl");
        std::fs::write(
            &stub,
            "#!/bin/sh\necho \"$@\" >> \"$LOG\"\nif [ \"$1\" = print ]; then exit \"$PRINT_RC\"; fi\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
            .env("LOG", &log)
            .env("PRINT_RC", if loaded { "0" } else { "113" })
            .env("HOME", "/home/test")
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn a_loaded_label_is_kickstarted_not_bootstrapped() {
        let log = run(&load_command(false), true);
        assert!(log.iter().any(|l| l.starts_with("enable gui/")), "{log:?}");
        assert!(
            log.iter().any(|l| l.starts_with("kickstart -k gui/")),
            "{log:?}"
        );
        assert!(!log.iter().any(|l| l.starts_with("bootstrap")), "{log:?}");
    }

    #[test]
    fn an_unloaded_label_is_bootstrapped_from_the_plist() {
        let log = run(&load_command(false), false);
        let boot = log
            .iter()
            .find(|l| l.starts_with("bootstrap"))
            .expect("bootstrap");
        assert!(
            boot.ends_with("/home/test/Library/LaunchAgents/dev.notchling.daemon.plist"),
            "{boot}"
        );
        assert!(!log.iter().any(|l| l.starts_with("kickstart")), "{log:?}");
    }

    #[test]
    fn restart_does_nothing_when_the_label_is_not_loaded() {
        assert!(run(&restart_command(false), false)
            .iter()
            .all(|l| l.starts_with("print")));
        assert!(run(&restart_command(false), true)
            .iter()
            .any(|l| l.starts_with("kickstart -k gui/")));
    }
}
