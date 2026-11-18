use super::device_login::{DevicePrompt, PromptSink, WITHHELD};
use super::fakes::TempDir;
use super::registry::Machine;
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
    guard: GuardKnobs,
    before: Arc<std::sync::Mutex<Option<&'static str>>>,
    timeouts: Arc<std::sync::Mutex<Vec<Duration>>>,
}

enum Login {
    Done(i32),
    Hold,
    Said(i32, &'static str),
    Silent(i32, &'static str),
}

fn rig(login: Login, status: &'static str) -> Rig {
    rig_on(linux("m1"), login, status)
}

fn rig_on(machine: Machine, login: Login, status: &'static str) -> Rig {
    let audit = TempDir::new("tool-login");
    let mut setup = Setup::new(|_, _| Vec::new());
    setup.machines = vec![machine];
    setup.audit = Some(audit.join("audit.jsonl"));
    let guard = guard_rules(&setup.runner);
    let timeouts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let before: Arc<std::sync::Mutex<Option<&'static str>>> =
        Arc::new(std::sync::Mutex::new(Some(SIGNED_OUT)));
    let ran = Arc::new(AtomicBool::new(false));
    let t = timeouts.clone();
    let started = ran.clone();
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("--device-auth")),
        move |call| {
            started.store(true, Ordering::SeqCst);
            lock(&t).push(call.timeout);
            if let Login::Silent(code, err) = &login {
                return CommandResult::new(*code, "", err);
            }
            if let Some(sink) = &call.on_output {
                sink(Channel::Stdout, PLAIN);
            }
            match &login {
                Login::Silent(..) => unreachable!(),
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
    let (gate, seen_run) = (before.clone(), ran.clone());
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("login status")),
        move |_| match *lock(&gate) {
            Some(first) if !seen_run.load(Ordering::SeqCst) => CommandResult::ok(first),
            _ => CommandResult::ok(status),
        },
    );
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("jetski-standalone")),
        |call| {
            let script = call.argv.last().cloned().unwrap_or_default();
            if script.contains("find-generic-password") {
                CommandResult::ok("@@missing\n@@keychain\n")
            } else {
                CommandResult::ok("@@missing\n")
            }
        },
    );
    Rig {
        w: setup.build(),
        prompts: Arc::default(),
        audit,
        guard,
        before,
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
            NODE,
            self.sink(),
            Arc::new(AtomicBool::new(false)),
        )
    }

    fn check(&self, tool: Tool) -> super::tool_login::LoginCheck {
        *lock(&self.before) = None;
        self.w.engine.check_login("m1", tool)
    }

    fn login_ran(&self) -> bool {
        self.calls().iter().any(|c| c.contains("--device-auth"))
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

fn statuses(calls: &[String]) -> usize {
    calls.iter().filter(|c| c.contains("login status")).count()
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
        .rposition(|c| c.contains("login status"))
        .unwrap();
    assert!(login < status, "{calls:?}");
    assert_eq!(statuses(&calls), 2, "{calls:?}");
    let first = calls
        .iter()
        .position(|c| c.contains("login status"))
        .unwrap();
    assert!(first < login, "{calls:?}");
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
    assert!(written.contains(WITHHELD), "{written}");
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
    assert_eq!(statuses(&r.calls()), 1);
    assert!(!r.everything_written().contains(CODE));
}

#[test]
fn cancelling_ends_the_command_and_skips_the_status_check() {
    let r = rig(Login::Hold, SIGNED_IN);
    let cancel = Arc::new(AtomicBool::new(false));
    let out = std::thread::scope(|scope| {
        let flag = cancel.clone();
        let run = scope.spawn(|| {
            r.w.engine
                .sign_in_tool("m1", Tool::Codex, NODE, r.sink(), flag)
        });
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
    assert_eq!(statuses(&r.calls()), 1);
    assert!(!r.everything_written().contains(CODE));
    assert_eq!(r.w.engine.running("m1"), None);
}

#[test]
fn a_failing_command_after_the_prompt_says_only_the_exit_code() {
    let said = "error: code TEST-00000 expired at https://auth.openai.com/codex/device";
    let r = rig(Login::Said(1, said), SIGNED_OUT);
    let out = r.sign_in();
    assert!(!out.ok);
    assert!(
        out.message.contains("didn't sign in on m1"),
        "{}",
        out.message
    );
    assert!(out.message.contains("(exit 1)"), "{}", out.message);
    assert!(!out.message.contains("error:"), "{}", out.message);
    assert!(!out.message.contains(CODE), "{}", out.message);
    assert!(!out.message.contains("openai.com"), "{}", out.message);
    assert!(!r.everything_written().contains(CODE));
}

#[test]
fn a_failure_before_any_prompt_shows_a_scrubbed_error() {
    let r = rig(
        Login::Silent(255, "ssh: connect to host m1.test: Connection refused"),
        SIGNED_OUT,
    );
    let out = r.sign_in();
    assert!(!out.ok);
    assert!(
        out.message.contains("Connection refused"),
        "{}",
        out.message
    );
}

#[test]
fn a_busy_codex_or_another_node_or_an_unreadable_probe_stops_before_the_login() {
    let r = rig(Login::Done(0), SIGNED_IN);
    *lock(&r.guard.busy) = Some(2);
    let out = r.sign_in();
    assert!(!out.ok);
    assert!(out.message.contains("is running on m1"), "{}", out.message);
    assert!(!r.login_ran());
    *lock(&r.guard.busy) = None;
    assert!(r.sign_in().message.contains("Couldn't tell"));
    assert!(!r.login_ran());
    *lock(&r.guard.busy) = Some(0);
    *lock(&r.guard.node) = Some("fedcba9876543210".into());
    let out = r.sign_in();
    assert!(
        out.message.contains("not the machine on this card"),
        "{}",
        out.message
    );
    *lock(&r.guard.node) = None;
    assert!(r.sign_in().message.contains("Couldn't confirm"));
    assert!(!r.login_ran());
    assert!(lock(&r.prompts).is_empty());
}

#[test]
fn nothing_from_the_workers_codex_folder_is_ever_read_back() {
    let r = rig(Login::Done(0), SIGNED_IN);
    assert!(r.sign_in().ok);
    r.check(Tool::Codex);
    for call in r.w.runner.calls() {
        let text = call.text();
        for banned in ["config.toml", "self-update", "auth.json"] {
            assert!(!text.contains(banned), "{banned}: {text}");
        }
    }
}

#[test]
fn check_again_reports_signed_in_signed_out_and_unknown_apart() {
    let r = rig(Login::Done(0), SIGNED_IN);
    let check = r.check(Tool::Codex);
    assert!(check.outcome.ok);
    assert_eq!(check.signed, Some(true));
    let r = rig(Login::Done(0), SIGNED_OUT);
    assert_eq!(r.check(Tool::Codex).signed, Some(false));
    let r = rig(Login::Done(0), "API key configured\n@@exit=0\n");
    let check = r.check(Tool::Codex);
    assert_eq!(check.signed, None);
    assert!(!check.outcome.ok);
}

#[test]
fn check_again_on_a_mac_looks_in_the_keychain_for_antigravity() {
    let r = rig_on(mac("m1"), Login::Done(0), SIGNED_IN);
    let check = r.check(Tool::Agy);
    assert_eq!(check.signed, Some(true), "{}", check.outcome.message);
    let r = rig(Login::Done(0), SIGNED_IN);
    assert_eq!(r.check(Tool::Agy).signed, Some(false));
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
    let out = r.w.engine.sign_in_tool(
        "m1",
        Tool::Agy,
        NODE,
        r.sink(),
        Arc::new(AtomicBool::new(false)),
    );
    assert!(!out.ok);
    assert!(out.message.contains("m1"), "{}", out.message);
    assert!(out.message.contains("Check Again"), "{}", out.message);
    assert!(r.w.runner.calls().is_empty());
}

#[test]
fn check_again_reads_the_login_status_only() {
    let r = rig(Login::Done(0), SIGNED_OUT);
    let out = r.check(Tool::Codex).outcome;
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
        NODE,
        r.sink(),
        Arc::new(AtomicBool::new(false)),
    );
    assert!(!out.ok);
    assert!(r.w.runner.calls().is_empty());
}

#[test]
fn a_login_is_never_started_over_a_working_or_unreadable_one() {
    let r = rig(Login::Done(0), SIGNED_IN);
    *lock(&r.before) = Some(SIGNED_IN);
    let out = r.sign_in();
    assert!(!out.ok);
    assert!(out.message.contains("already signed in"), "{}", out.message);
    assert!(!r.login_ran());
    *lock(&r.before) = Some("API key configured\n@@exit=0\n");
    let out = r.sign_in();
    assert!(!out.ok);
    assert!(out.message.contains("Couldn't tell"), "{}", out.message);
    assert!(
        out.message.contains("Nothing was changed"),
        "{}",
        out.message
    );
    assert!(!r.login_ran());
    assert!(lock(&r.prompts).is_empty());
}
