use super::*;
use crate::delegate::test_support::{rig, ShScript};
use crate::tools::acp::fake::FakeSpawner as AcpFake;
use crate::tools::codex::fake::FakeRun;
use std::path::PathBuf;
use std::time::Duration;

const STARTED: &str = r#"{"type":"thread.started","thread_id":"019e-session-1"}"#;
const MESSAGE: &str = r#"{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"Line 3 looks off."}}"#;
const DONE: &str = r#"{"type":"turn.completed","usage":{"input_tokens":800,"cached_input_tokens":5,"output_tokens":100}}"#;

fn contains(args: &[String], pair: &[&str]) -> bool {
    args.windows(pair.len())
        .any(|w| w.iter().zip(pair).all(|(a, b)| a == b))
}

#[test]
fn a_read_only_turn_runs_codex_in_its_read_only_sandbox_with_no_network() {
    let rig = rig(AcpFake::new());
    rig.codex
        .push(FakeRun::ok(&[STARTED, "not json", MESSAGE, DONE]));
    let mut options = rig.options(Tool::Codex, "review this");
    options.model = Some("gpt-x".into());
    options.effort = Some("high".into());
    let outcome = run(&options, &rig.deps);

    assert_eq!(outcome.error, None);
    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(outcome.reply, "Line 3 looks off.");
    assert_eq!(outcome.session.as_deref(), Some("019e-session-1"));
    assert_eq!(outcome.tokens, Some(900));
    assert_eq!(outcome.model.as_deref(), Some("gpt-x"));

    let spec = rig.codex.specs().remove(0);
    assert_eq!(spec.program, PathBuf::from("/fake/codex"));
    assert_eq!(spec.cwd, options.folder);
    assert_eq!(spec.args[0], "exec");
    assert!(contains(&spec.args, &["-c", "sandbox_mode=\"read-only\""]));
    assert!(
        !spec.args.iter().any(|a| a.contains("network_access")),
        "the read-only sandbox never opens the network: {:?}",
        spec.args
    );
    assert!(contains(&spec.args, &["-m", "gpt-x"]));
    assert!(contains(
        &spec.args,
        &["-c", "model_reasoning_effort=\"high\""]
    ));
    assert!(contains(&spec.args, &["-C", &options.folder]));
    assert_eq!(spec.args.last().map(String::as_str), Some("review this"));
    assert!(spec.env.contains(&(
        "NOTCHLING_DELEGATE_DEPTH".to_string(),
        Some("1".to_string())
    )));
    assert!(spec
        .env
        .contains(&("PATH".to_string(), Some("/fake/bin".to_string()))));
}

#[test]
fn write_uses_the_workspace_sandbox_and_the_network_only_when_allowed() {
    let mut rig = rig(AcpFake::new());
    rig.codex.push(FakeRun::ok(&[STARTED, MESSAGE]));
    let mut options = rig.options(Tool::Codex, "change it");
    options.write = true;
    run(&options, &rig.deps);
    let args = rig.codex.specs().remove(0).args;
    assert!(contains(&args, &["-c", "sandbox_mode=\"workspace-write\""]));
    assert!(contains(
        &args,
        &["-c", "sandbox_workspace_write.network_access=true"]
    ));

    rig.deps.network = false;
    rig.codex.push(FakeRun::ok(&[STARTED, MESSAGE]));
    run(&options, &rig.deps);
    let args = rig.codex.specs().remove(1).args;
    assert!(args.iter().all(|a| !a.contains("network_access")));
}

#[test]
fn a_session_continues_with_resume() {
    let rig = rig(AcpFake::new());
    rig.codex.push(FakeRun::ok(&[MESSAGE]));
    let mut options = rig.options(Tool::Codex, "and then?");
    options.session = Some("019e-abc".into());
    let outcome = run(&options, &rig.deps);
    let args = rig.codex.specs().remove(0).args;
    assert_eq!(&args[..2], ["exec", "resume"]);
    assert!(!args.contains(&"-C".to_string()), "resume keeps its folder");
    assert_eq!(args[args.len() - 2], "019e-abc");
    assert_eq!(
        outcome.session.as_deref(),
        Some("019e-abc"),
        "the session stays when thread.started is not printed"
    );
}

#[test]
fn a_failed_turn_says_what_it_printed_last() {
    let rig = rig(AcpFake::new());
    rig.codex.push(
        FakeRun::ok(&[
            STARTED,
            r#"{"type":"error","message":"stream disconnected"}"#,
            r#"{"type":"turn.failed","error":{"message":"You exceeded your usage limit."}}"#,
        ])
        .stderr("2026 ERROR something else")
        .exiting(1),
    );
    let outcome = run(&rig.options(Tool::Codex, "x"), &rig.deps);
    assert_eq!(
        outcome.error.as_deref(),
        Some("You exceeded your usage limit.")
    );
    assert_eq!(outcome.exit_code(), 1);

    rig.codex.push(
        FakeRun::ok(&[])
            .stderr("first line")
            .stderr("fatal error: boom")
            .exiting(3),
    );
    let outcome = run(&rig.options(Tool::Codex, "x"), &rig.deps);
    assert_eq!(outcome.error.as_deref(), Some("fatal error: boom"));

    rig.codex.push(FakeRun::ok(&[]).exiting(4));
    let outcome = run(&rig.options(Tool::Codex, "x"), &rig.deps);
    assert_eq!(outcome.error.as_deref(), Some("Codex exited with code 4."));
}

#[test]
fn it_cannot_start_without_the_binary_or_with_a_spawn_error_or_odd_arguments() {
    let mut rig = rig(AcpFake::new());
    rig.codex.push_error("permission denied");
    let o = run(&rig.options(Tool::Codex, "x"), &rig.deps);
    assert_eq!(o.error.as_deref(), Some("permission denied"));

    let mut odd = rig.options(Tool::Codex, "x");
    odd.effort = Some("high\" -c evil=\"1".into());
    assert!(run(&odd, &rig.deps).error.unwrap().contains("--effort"));
    odd.effort = None;
    odd.session = Some("--help".into());
    assert!(run(&odd, &rig.deps).error.unwrap().contains("session id"));
    assert!(rig.codex.specs().is_empty(), "nothing was started");

    rig.deps.codex_binary = None;
    let o = run(&rig.options(Tool::Codex, "x"), &rig.deps);
    assert_eq!(o.error.as_deref(), Some(NO_BINARY));
    assert_eq!(o.exit_code(), 1);
}

#[test]
fn media_made_during_the_turn_is_listed_and_older_files_are_not() {
    let rig = rig(AcpFake::new());
    let folder = rig.codex_media().join("019e-session-1");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("cup.png"), "x").unwrap();
    std::fs::write(folder.join("old.png"), "x").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(folder.join("old.png"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    std::fs::write(folder.join("notes.md"), "x").unwrap();
    rig.codex.push(FakeRun::ok(&[STARTED, MESSAGE]));
    let outcome = run(&rig.options(Tool::Codex, "draw"), &rig.deps);
    assert_eq!(outcome.media, [folder.join("cup.png").to_string_lossy()]);
}

#[test]
fn a_session_id_cannot_point_the_media_pickup_out_of_the_folder() {
    let rig = rig(AcpFake::new());
    let secret = rig.root.join("secret");
    std::fs::create_dir_all(&secret).unwrap();
    std::fs::write(secret.join("loot.png"), "x").unwrap();
    // Codex "names" a session with dots in it.
    rig.codex.push(FakeRun::ok(&[
        r#"{"type":"thread.started","thread_id":"../../secret"}"#,
        MESSAGE,
    ]));
    let outcome = run(&rig.options(Tool::Codex, "x"), &rig.deps);
    assert!(outcome.media.is_empty(), "{:?}", outcome.media);
}

#[test]
fn the_timeout_interrupts_the_turn_and_exits_124() {
    let rig = rig(AcpFake::new());
    rig.codex.push(FakeRun::held(&[STARTED]));
    let mut options = rig.options(Tool::Codex, "slow");
    options.timeout = Duration::from_millis(150);
    let outcome = run(&options, &rig.deps);
    assert!(outcome.timed_out);
    assert_eq!(outcome.exit_code(), 124);
    assert_eq!(outcome.error.as_deref(), Some("Stopped after 0 s."));
    assert_eq!(rig.codex.processes()[0].interrupts(), 1);
    assert_eq!(outcome.session.as_deref(), Some("019e-session-1"));
}

#[test]
fn a_stop_signal_interrupts_the_turn_but_is_not_a_timeout() {
    let rig = rig(AcpFake::new());
    rig.codex.push(FakeRun::held(&[STARTED, MESSAGE]));
    let stop = rig.deps.stop.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        stop.store(true, Ordering::SeqCst);
    });
    let outcome = run(&rig.options(Tool::Codex, "slow"), &rig.deps);
    stopper.join().unwrap();
    assert!(!outcome.timed_out);
    assert_eq!(outcome.error.as_deref(), Some("Codex was stopped."));
    assert_eq!(outcome.exit_code(), 1);
    assert_eq!(outcome.reply, "Line 3 looks off.", "what it said so far");
}

#[test]
fn a_turn_that_ignores_the_interrupt_and_the_terminate_is_killed_so_the_timeout_holds() {
    let rig = rig(AcpFake::new());
    rig.codex.push(FakeRun::stubborn(&[STARTED]));
    let mut options = rig.options(Tool::Codex, "stubborn");
    options.timeout = Duration::from_millis(100);
    let deps = rig.deps.clone();
    let (done, outcome) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(run(&options, &deps));
    });
    let outcome = outcome.recv_timeout(Duration::from_secs(5));
    let process = rig.codex.processes().remove(0);
    // A run that never ends is let go, so a failure says so instead of hanging the suite.
    process.release();
    let outcome = outcome.expect("the run ended without a release: it was killed");
    assert!(outcome.timed_out);
    assert_eq!(
        process.kills(),
        1,
        "SIGKILL after the SIGTERM went unheeded"
    );
    assert!(process.interrupts() >= 2, "interrupted, then terminated");
}

#[cfg(unix)]
#[test]
fn a_turn_that_ignores_the_interrupt_is_terminated_a_little_later() {
    let mut rig = rig(AcpFake::new());
    rig.deps.codex = Arc::new(ShScript {
        script: format!("trap '' INT; echo '{STARTED}'; exec sleep 30"),
    });
    let mut options = rig.options(Tool::Codex, "stubborn");
    options.timeout = Duration::from_millis(100);
    let began = Instant::now();
    let outcome = run(&options, &rig.deps);
    assert!(outcome.timed_out);
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "it did not wait for the sleep"
    );
    assert_eq!(outcome.session.as_deref(), Some("019e-session-1"));
}

#[cfg(unix)]
#[test]
fn a_real_turn_that_traps_sigint_and_sigterm_is_killed() {
    let mut rig = rig(AcpFake::new());
    rig.deps.codex = Arc::new(ShScript {
        script: format!("trap '' INT TERM; echo '{STARTED}'; exec sleep 30"),
    });
    let mut options = rig.options(Tool::Codex, "deaf");
    options.timeout = Duration::from_millis(100);
    let began = Instant::now();
    let outcome = run(&options, &rig.deps);
    assert!(outcome.timed_out);
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "it did not wait for the sleep"
    );
}

#[cfg(unix)]
#[test]
fn a_real_process_is_read_line_by_line() {
    let mut rig = rig(AcpFake::new());
    rig.deps.codex = Arc::new(ShScript {
        script: format!("echo '{STARTED}'; echo '{MESSAGE}'; echo '{DONE}'; echo 'warn error: late' >&2; exit 0"),
    });
    let outcome = run(&rig.options(Tool::Codex, "x"), &rig.deps);
    assert_eq!(
        outcome.error, None,
        "an exit 0 with an error line on stderr is done"
    );
    assert_eq!(outcome.reply, "Line 3 looks off.");
    assert_eq!(outcome.tokens, Some(900));
}
