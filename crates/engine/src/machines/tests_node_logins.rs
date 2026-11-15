use super::catalog;
use super::fakes_b::*;
use super::runner::{Call, CommandResult, Runner};
use super::shell;
use super::step::{CheckState, StepFix, StepId};
use super::tests_node_steps::{node_linux, node_mac, probe, step};
use crate::cluster::logins::argv_read_only;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const CODEX: StepId = StepId::NODE_CODEX_LOGIN;
const AGY: StepId = StepId::NODE_AGY_LOGIN;

fn codex(text: &str, exit: i32) -> String {
    format!("{text}\n@@exit={exit}\n")
}

#[test]
fn codex_step_reads_signed_in_signed_out_and_unknown() {
    let rig = Rig::new(node_linux());
    let on = probe(&rig, &CODEX, &codex("Logged in using ChatGPT", 0), 0);
    assert_eq!(on, CheckState::Pass("signed in".to_string()));
    let off = probe(&rig, &CODEX, &codex("Not logged in", 1), 0);
    assert_eq!(off, CheckState::Fail("not signed in".to_string()));
    for text in [
        "@@none\n".to_string(),
        "sh: codex: not found".to_string(),
        codex("error: unexpected argument", 2),
        codex("", 124),
    ] {
        let state = probe(&rig, &CODEX, &text, 0);
        assert!(
            matches!(state, CheckState::CantTell(_)),
            "{text}: {state:?}"
        );
    }
}

#[test]
fn the_codex_pass_text_never_echoes_what_codex_said() {
    let rig = Rig::new(node_linux());
    let said = "Logged in using an API key - sk-NL-MARKER-8";
    let state = probe(&rig, &CODEX, &codex(said, 0), 0);
    assert!(!state.text().contains("NL-MARKER"), "{state:?}");
    let off = probe(&rig, &CODEX, &codex("Not logged in sk-NL-MARKER-8", 1), 0);
    assert!(!off.text().contains("NL-MARKER"), "{off:?}");
}

#[test]
fn agy_step_reads_the_login_file() {
    let linux = Rig::new(node_linux());
    assert!(probe(&linux, &AGY, "@@present\n", 0).is_pass());
    assert!(probe(&linux, &AGY, "@@missing\n", 0).is_fail());
    for text in ["@@none\n", "", "garbage"] {
        let state = probe(&linux, &AGY, text, 0);
        assert!(
            matches!(state, CheckState::CantTell(_)),
            "{text}: {state:?}"
        );
    }
    let mac = Rig::new(node_mac());
    assert!(probe(&mac, &AGY, "@@present\n", 0).is_pass());
    let state = probe(&mac, &AGY, "@@missing\n", 0);
    assert!(matches!(state, CheckState::CantTell(_)), "{state:?}");
}

#[test]
fn machines_login_steps_are_howto() {
    let m = node_linux();
    let codex = step(&m, &CODEX);
    let agy = step(&m, &AGY);
    match (&codex.fix, &agy.fix) {
        (StepFix::HowTo(c), StepFix::HowTo(a)) => {
            assert!(c.contains("codex login --device-auth"), "{c}");
            assert!(c.contains("own terminal"), "{c}");
            assert!(a.contains("own terminal"), "{a}");
            assert!(c.contains(&m.name) && a.contains(&m.name));
        }
        _ => panic!("both are a how-to"),
    }
    assert!(codex.plan().is_none() && agy.plan().is_none());
    assert!(codex.id.is_node() && agy.id.is_node());
}

#[test]
fn the_steps_exist_only_for_a_cluster_node() {
    let node = catalog::steps(&node_linux(), &[]);
    assert!(node.iter().any(|s| s.id == CODEX));
    assert!(node.iter().any(|s| s.id == AGY));
    let mut plain = node_linux();
    plain.node = None;
    let plain = catalog::steps(&plain, &[]);
    assert!(!plain.iter().any(|s| s.id == CODEX || s.id == AGY));
}

struct Guard {
    hit: Arc<AtomicBool>,
    inner: super::runner::ScriptedRunner,
}

impl Runner for Guard {
    fn run(&self, call: &Call) -> CommandResult {
        if !argv_read_only(&call.argv) {
            self.hit.store(true, Ordering::SeqCst);
            return CommandResult::refused("refused: a login would be written");
        }
        self.inner.run(call)
    }
}

#[test]
fn probes_never_refresh() {
    let m = node_linux();
    let hit = Arc::new(AtomicBool::new(false));
    let guard = Guard {
        hit: hit.clone(),
        inner: super::runner::ScriptedRunner::new(),
    };
    for id in [CODEX, AGY] {
        let found = step(&m, &id);
        let (script, _) = remote(&found);
        let call = Call::secs(shell::ssh_script(&m, &script), 8);
        let ran = guard.run(&call);
        assert_eq!(ran.code, 0, "{id:?}: {}", ran.err);
        assert!(!script.contains("setup-token") && !script.contains("logout"));
        assert!(!script.contains("refresh"));
    }
    assert!(!hit.load(Ordering::SeqCst));
    let bad = Call::secs(shell::ssh_script(&m, "codex login --device-auth"), 8);
    assert_eq!(guard.run(&bad).code, 126);
    assert!(hit.load(Ordering::SeqCst));
}

#[test]
fn the_probes_read_the_file_without_opening_it() {
    let (script, _) = remote(&step(&node_linux(), &AGY));
    assert!(script.contains("[ -s "), "{script}");
    for word in ["cat ", "<", "base64", "head", "tail", "grep"] {
        assert!(!script.contains(word), "{word}: {script}");
    }
    let (codex, _) = remote(&step(&node_linux(), &CODEX));
    assert!(codex.contains("codex login status"));
    assert!(!codex.contains("auth.json"));
}

#[cfg(unix)]
mod on_this_machine {
    use super::*;
    use crate::machines::steps_node_logins::{codex_script, WAIT_TENTHS};
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::time::{Duration, Instant};

    fn run_with(codex: Option<&str>, tenths: u32) -> (String, Duration) {
        let dir =
            std::env::temp_dir().join(format!("nl-codex-probe-{}-{}", std::process::id(), tenths));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if let Some(body) = codex {
            let path = dir.join("codex");
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let started = Instant::now();
        let out = Command::new("/bin/sh")
            .arg("-c")
            .arg(codex_script(tenths))
            .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let _ = std::fs::remove_dir_all(&dir);
        (text, started.elapsed())
    }

    #[test]
    fn the_script_reports_the_exit_code_and_the_text() {
        let rig = Rig::new(node_linux());
        let (on, _) = run_with(Some("echo 'Logged in using ChatGPT'"), 20);
        assert!(probe(&rig, &CODEX, &on, 0).is_pass(), "{on}");
        let (off, _) = run_with(Some("echo 'Not logged in'; exit 1"), 20);
        assert!(probe(&rig, &CODEX, &off, 0).is_fail(), "{off}");
        let (none, _) = run_with(None, 20);
        assert!(matches!(
            probe(&rig, &CODEX, &none, 0),
            CheckState::CantTell(_)
        ));
    }

    #[test]
    fn a_hung_codex_is_cut_and_reads_as_cant_tell_for_this_step_only() {
        let rig = Rig::new(node_linux());
        let (text, took) = run_with(Some("sleep 30"), 3);
        assert!(text.contains("@@timeout"), "{text}");
        assert!(took < Duration::from_secs(5), "{took:?}");
        let state = probe(&rig, &CODEX, &text, 0);
        assert!(matches!(state, CheckState::CantTell(_)), "{state:?}");
        const { assert!(WAIT_TENTHS <= 50) };
        assert!(crate::cluster::logins::read_only(&codex_script(
            WAIT_TENTHS
        )));
    }
}
