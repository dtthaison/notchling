use super::device_login::{DevicePrompt, PromptSink};
use super::fakes::TempDir;
use super::runner::{Channel, CommandResult, CANCELLED, TIMED_OUT};
use super::tests_engine_world::*;
use super::tool_login::{can_sign_in, login_script, LOGIN_SECS};
use crate::versions::Tool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const PLAIN: &str = include_str!("../../tests/fixtures/codex-login/device-auth.txt");
const URL: &str = "https://auth.openai.com/codex/device";
const CODE: &str = "TEST-00000";

type Prompts = Arc<std::sync::Mutex<Vec<DevicePrompt>>>;

struct Rig {
    w: World,
    prompts: Prompts,
    audit: TempDir,
    timeouts: Arc<std::sync::Mutex<Vec<Duration>>>,
}

enum Login {
    Done(i32),
    Hold,
    Said(i32, &'static str),
}

fn rig(login: Login, status: &'static str) -> Rig {
    let audit = TempDir::new("tool-login");
    let mut setup = Setup::new(|_, _| Vec::new());
    setup.machines = vec![linux("m1")];
    setup.audit = Some(audit.join("audit.jsonl"));
    let timeouts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let t = timeouts.clone();
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("--device-auth")),
        move |call| {
            lock(&t).push(call.timeout);
            if let Some(sink) = &call.on_output {
                sink(Channel::Stdout, PLAIN);
            }
            match &login {
                Login::Done(code) => CommandResult::new(*code, PLAIN, ""),
                Login::Said(code, err) => CommandResult::new(*code, PLAIN, err),
                Login::Hold => {
                    let flag = call.cancel.clone().unwrap();
                    let until = std::time::Instant::now() + Duration::from_secs(10);
                    while !flag.load(Ordering::SeqCst) && std::time::Instant::now() < until {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    CommandResult::new(CANCELLED, "", "")
                }
            }
        },
    );
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("login status")),
        move |_| CommandResult::ok(status),
    );
    Rig {
        w: setup.build(),
        prompts: Arc::default(),
        audit,
        timeouts,
    }
}

impl Rig {
    fn sink(&self) -> PromptSink {
        let seen = self.prompts.clone();
        Arc::new(move |p| lock(&seen).push(p))
    }

    fn sign_in(&self) -> super::step::FixOutcome {
        self.w.engine.sign_in_tool(
            "m1",
            Tool::Codex,
            self.sink(),
            Arc::new(AtomicBool::new(false)),
        )
    }

    fn calls(&self) -> Vec<String> {
        self.w
            .runner
            .calls()
            .into_iter()
            .map(|c| c.argv.last().cloned().unwrap_or_default())
            .collect()
    }

    fn everything_written(&self) -> String {
        let audit = std::fs::read_to_string(self.audit.join("audit.jsonl")).unwrap_or_default();
        format!("{}\n{audit}", self.w.log("m1"))
    }
}

const SIGNED_IN: &str = "Logged in using ChatGPT\n@@exit=0\n";
const SIGNED_OUT: &str = "Not logged in\n@@exit=1\n";

#[test]
fn success_needs_the_status_check_after_the_login_command() {
    let r = rig(Login::Done(0), SIGNED_IN);
    let out = r.sign_in();
    assert!(out.ok, "{}", out.message);
    assert_eq!(out.message, "Codex is signed in on m1");
    let calls = r.calls();
    let login = calls
        .iter()
        .position(|c| c.contains("--device-auth"))
        .unwrap();
    let status = calls
        .iter()
        .position(|c| c.contains("login status"))
        .unwrap();
    assert!(login < status, "{calls:?}");
}

#[test]
fn the_prompt_reaches_the_caller_and_nothing_else() {
    let r = rig(Login::Done(0), SIGNED_IN);
    let out = r.sign_in();
    let seen = lock(&r.prompts);
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].url(), URL);
    assert_eq!(seen[0].code(), CODE);
    let written = r.everything_written();
    assert!(!written.contains(CODE), "{written}");
    assert!(!written.contains("auth.openai.com"), "{written}");
    assert!(!out.message.contains(CODE) && !out.message.contains("openai"));
    assert!(written.contains("$ codex login --device-auth"), "{written}");
    assert!(written.contains("[one-time code]"), "{written}");
    assert!(written.contains("✓ Codex is signed in on m1"), "{written}");
    let audit = std::fs::read_to_string(r.audit.join("audit.jsonl")).unwrap();
    assert!(audit.contains("\"op\":\"machines.fix\""), "{audit}");
    assert!(audit.contains("Sign in Codex on m1"), "{audit}");
}

#[test]
fn a_clean_exit_is_not_a_success_while_the_status_says_signed_out() {
    let r = rig(Login::Done(0), SIGNED_OUT);
    let out = r.sign_in();
    assert!(!out.ok);
    assert!(
        out.message.contains("isn't signed in on m1"),
        "{}",
        out.message
    );
    let r = rig(Login::Done(0), "error: unexpected argument\n@@exit=2\n");
    let out = r.sign_in();
    assert!(!out.ok);
    assert!(out.message.contains("Couldn't tell"), "{}", out.message);
}

#[test]
fn the_wait_is_bounded_to_ten_minutes_and_a_timeout_says_so() {
    let r = rig(Login::Done(TIMED_OUT), SIGNED_IN);
    let out = r.sign_in();
    assert!(!out.ok);
    assert!(out.message.contains("within 10 minutes"), "{}", out.message);
    assert_eq!(
        lock(&r.timeouts).clone(),
        vec![Duration::from_secs(LOGIN_SECS)]
    );
    assert_eq!(LOGIN_SECS, 600);
    assert!(!r.calls().iter().any(|c| c.contains("login status")));
    assert!(!r.everything_written().contains(CODE));
}

#[test]
fn cancelling_ends_the_command_and_skips_the_status_check() {
    let r = rig(Login::Hold, SIGNED_IN);
    let cancel = Arc::new(AtomicBool::new(false));
    let out = std::thread::scope(|scope| {
        let flag = cancel.clone();
        let run = scope.spawn(|| r.w.engine.sign_in_tool("m1", Tool::Codex, r.sink(), flag));
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while lock(&r.prompts).is_empty() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(lock(&r.prompts).len(), 1);
        cancel.store(true, Ordering::SeqCst);
        run.join().unwrap()
    });
    assert!(!out.ok);
    assert!(out.message.contains("was cancelled"), "{}", out.message);
    assert!(!r.calls().iter().any(|c| c.contains("login status")));
    assert!(!r.everything_written().contains(CODE));
    assert_eq!(r.w.engine.running("m1"), None);
}

#[test]
fn a_failing_command_shows_a_scrubbed_error() {
    let said = "error: code TEST-00000 expired at https://auth.openai.com/codex/device";
    let r = rig(Login::Said(1, said), SIGNED_OUT);
    let out = r.sign_in();
    assert!(!out.ok);
    assert!(
        out.message.contains("didn't sign in on m1"),
        "{}",
        out.message
    );
    assert!(out.message.contains("error:"), "{}", out.message);
    assert!(!out.message.contains(CODE), "{}", out.message);
    assert!(!out.message.contains("openai.com"), "{}", out.message);
    assert!(!r.everything_written().contains(CODE));
}

#[test]
fn only_a_device_login_over_ssh_runs_and_nothing_touches_auth_files() {
    let r = rig(Login::Done(0), SIGNED_IN);
    r.sign_in();
    for call in r.w.runner.calls() {
        assert_eq!(call.argv[0], "/usr/bin/ssh", "{:?}", call.argv);
        assert!(call
            .argv
            .iter()
            .any(|a| a == "m1.test" || a.ends_with("@m1.test")));
        let text = call.text();
        for banned in [
            "logout",
            "refresh",
            "auth.json",
            "cat ",
            "scp",
            "rsync",
            "sudo",
        ] {
            assert!(!text.contains(banned), "{banned}: {text}");
        }
    }
    let script = login_script();
    assert!(script.contains("codex login --device-auth"));
    assert!(!script.contains(".codex"));
}

#[test]
fn antigravity_has_no_remote_sign_in_and_gets_a_how_to() {
    let r = rig(Login::Done(0), SIGNED_IN);
    assert!(!can_sign_in(Tool::Agy));
    let out =
        r.w.engine
            .sign_in_tool("m1", Tool::Agy, r.sink(), Arc::new(AtomicBool::new(false)));
    assert!(!out.ok);
    assert!(out.message.contains("m1"), "{}", out.message);
    assert!(out.message.contains("Check Again"), "{}", out.message);
    assert!(r.w.runner.calls().is_empty());
}

#[test]
fn check_again_reads_the_login_status_only() {
    let r = rig(Login::Done(0), SIGNED_OUT);
    let out = r.w.engine.check_login("m1", Tool::Codex);
    assert!(!out.ok);
    assert!(!r.calls().iter().any(|c| c.contains("--device-auth")));
    assert!(r.calls().iter().any(|c| c.contains("login status")));
}

#[test]
fn a_machine_that_is_not_registered_is_refused() {
    let r = rig(Login::Done(0), SIGNED_IN);
    let out = r.w.engine.sign_in_tool(
        "nobody",
        Tool::Codex,
        r.sink(),
        Arc::new(AtomicBool::new(false)),
    );
    assert!(!out.ok);
    assert!(r.w.runner.calls().is_empty());
}
