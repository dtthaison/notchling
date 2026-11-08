//! Steps 1-6 and 8 of the catalog (Swift `MachinesMachineStepsTests`): each probe's parse, and each
//! Fix on a scripted runner. No ssh starts.

use super::fakes::*;
use super::registry::{DockerState, Machine, MachineOs};
use super::runner::{CommandResult, ScriptedRunner};
use super::step::{
    CheckResult, CheckState, FixOutcome, FixPlan, Probe, RemoteOutput, SetupStep, StepFix, StepId,
};
use super::steps_machine::all;
use std::sync::Arc;

pub(super) fn mac() -> Machine {
    TestMachines::mac("m1")
}

pub(super) fn linux() -> Machine {
    TestMachines::linux("vps-sing-1")
}

pub(super) fn step(id: StepId, machine: &Machine) -> SetupStep {
    all(machine)
        .into_iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("no step {}", id.raw()))
}

pub(super) fn script(s: &SetupStep) -> String {
    s.probe.script().unwrap_or_default().to_string()
}

pub(super) fn parse(id: StepId, text: &str, code: i32, machine: &Machine) -> CheckResult {
    parse_in(id, text, code, &Ctx::new(machine.clone()))
}

pub(super) fn parse_in(id: StepId, text: &str, code: i32, ctx: &Ctx) -> CheckResult {
    let s = step(id, &ctx.machine);
    let Probe::Remote { parse, .. } = &s.probe else {
        panic!("no remote probe");
    };
    parse(
        &RemoteOutput {
            text: text.into(),
            code,
        },
        &ctx.step(),
    )
}

pub(super) fn check(id: StepId, text: &str) -> CheckResult {
    parse(id, text, 0, &mac())
}

pub(super) fn plan(s: &SetupStep) -> FixPlan {
    match &s.fix {
        StepFix::Run(p) => p.clone(),
        _ => panic!("{} has no Fix", s.id.raw()),
    }
}

pub(super) fn how_to(s: &SetupStep) -> String {
    match &s.fix {
        StepFix::HowTo(t) => t.clone(),
        _ => panic!("{} is not a how-to", s.id.raw()),
    }
}

pub(super) fn run_fix(p: &FixPlan, ctx: &Ctx, admin: Option<&str>) -> FixOutcome {
    let (fix, _) = ctx.fix(admin);
    (p.run)(&fix)
}

pub(super) fn failing_runner(code: i32, out: &str, err: &str) -> Arc<ScriptedRunner> {
    let runner = Arc::new(ScriptedRunner::new());
    runner.on(|_| true, CommandResult::new(code, out, err));
    runner
}

// MARK: Reachable

#[test]
fn reachable_parses_os_arch_home_version_disk_and_docker() {
    let out = "os=Darwin\narch=arm64\nuid=501\nuser=sondoan\nhome=/Users/sondoan\nversion=26.1\ndisk_kb=125829120\ndocker=rootless\n";
    let r = check(StepId::REACHABLE, out);
    assert_eq!(r.state, CheckState::Pass("macOS 26.1, arm64".into()));
    assert_eq!(r.detected.os, Some(MachineOs::MacOs));
    assert_eq!(r.detected.home.as_deref(), Some("/Users/sondoan"));
    let a = r.detected.abilities.unwrap();
    assert_eq!(a.free_disk_gb, Some(120));
    assert_eq!(a.docker, Some(DockerState::Rootless));
    assert_eq!(a.arch.as_deref(), Some("arm64"));
    assert_eq!(a.os_version.as_deref(), Some("26.1"));
    assert_eq!(a.home.as_deref(), Some("/Users/sondoan"));
}

#[test]
fn reachable_parses_linux_and_keeps_what_other_steps_found() {
    let mut machine = linux();
    machine.abilities.as_mut().unwrap().xcode = Some("kept".into());
    let out = "os=Linux\narch=x86_64\nuid=1001\nuser=agent\nhome=/home/agent\nversion=Ubuntu 24.04.1 LTS\ndisk_kb=2097152\ndocker=none\n";
    let r = parse(StepId::REACHABLE, out, 0, &machine);
    assert_eq!(
        r.state,
        CheckState::Pass("Ubuntu 24.04.1 LTS, x86_64".into())
    );
    assert_eq!(r.detected.os, Some(MachineOs::Linux));
    let a = r.detected.abilities.unwrap();
    assert_eq!(a.docker, Some(DockerState::None));
    assert_eq!(a.free_disk_gb, Some(2));
    assert_eq!(a.xcode.as_deref(), Some("kept"));
}

#[test]
fn reachable_keys_each_value_by_its_name_not_its_position() {
    // a line missing and a banner in front: nothing shifts
    let out = "Welcome to the machine\nos=Linux\nhome=/home/agent\narch=aarch64\nversion=Debian 12\ndocker=stopped\n";
    let r = parse(StepId::REACHABLE, out, 0, &linux());
    assert_eq!(r.state, CheckState::Pass("Debian 12, aarch64".into()));
    assert_eq!(r.detected.home.as_deref(), Some("/home/agent"));
    let a = r.detected.abilities.unwrap();
    assert_eq!(a.docker, Some(DockerState::Stopped));
    assert_eq!(a.free_disk_gb, None);
}

#[test]
fn reachable_refuses_a_home_with_shell_characters() {
    for home in [
        "/home/a;touch x",
        "/home/$(id)",
        "relative/home",
        "/home/a/../b",
        "/home/a b",
        "/home/`id`",
        "/home/a'b",
    ] {
        let out = format!("os=Linux\narch=x86_64\nhome={home}\nversion=Ubuntu\n");
        let r = parse(StepId::REACHABLE, &out, 0, &linux());
        let CheckState::Fail(text) = &r.state else {
            panic!("{home} did not fail");
        };
        assert!(text.contains("home folder"), "{text}");
        assert!(!text.contains(home), "the hostile text is not echoed");
        assert_eq!(r.detected.home, None, "{home}");
        assert_eq!(
            r.detected.abilities.as_ref().and_then(|a| a.home.clone()),
            None,
            "{home}"
        );
    }
    let missing = parse(StepId::REACHABLE, "os=Linux\narch=x86_64\n", 0, &linux());
    assert!(missing.state.is_fail(), "no home reported should fail");
}

#[test]
fn reachable_refuses_an_unknown_os() {
    let r = check(StepId::REACHABLE, "os=FreeBSD\narch=amd64\nhome=/home/a\n");
    assert_eq!(r.state, CheckState::Fail("unsupported OS FreeBSD".into()));
    assert_eq!(r.detected.os, None);
}

#[test]
fn reachable_with_no_answer_is_a_failure_or_cant_tell() {
    let down = parse(
        StepId::REACHABLE,
        "ssh: connect to host m1 port 22: Operation timed out\n",
        255,
        &mac(),
    );
    assert!(down.state.is_fail(), "a failed ssh is a fail");
    let odd = check(StepId::REACHABLE, "");
    assert!(matches!(odd.state, CheckState::CantTell(_)));
}

#[test]
fn reachable_cuts_long_machine_strings() {
    let long = "x".repeat(500);
    let out = format!("os=Linux\narch=x86_64\nhome=/home/a\nversion={long}\n");
    let r = parse(StepId::REACHABLE, &out, 0, &linux());
    let version = r.detected.abilities.unwrap().os_version.unwrap();
    assert_eq!(version.chars().count(), 200);
}

#[test]
fn reachable_probe_is_read_only_and_prints_every_key() {
    let s = script(&step(StepId::REACHABLE, &mac()));
    for key in [
        "os=", "arch=", "uid=", "user=", "home=", "version=", "disk_kb=", "docker=",
    ] {
        assert!(s.contains(key), "{key}");
    }
    assert!(s.contains("docker info --format '{{json .SecurityOptions}}'"));
    for word in ["sudo", "rm ", "install", "> "] {
        assert!(!s.contains(word), "{word}");
    }
    let how = how_to(&step(StepId::REACHABLE, &mac()));
    assert!(how.contains("tailscale ping"));
    assert!(how.contains("ACL"));
}

// MARK: Worker user

#[test]
fn worker_user_fails_for_root() {
    let root = check(StepId::WORKER_USER, "0\n");
    let CheckState::Fail(text) = &root.state else {
        panic!("root should fail");
    };
    assert!(text.contains("root"), "{text}");
    assert!(check(StepId::WORKER_USER, "1001\n").state.is_pass());
    assert!(matches!(
        check(StepId::WORKER_USER, "").state,
        CheckState::CantTell(_)
    ));
    assert!(how_to(&step(StepId::WORKER_USER, &mac())).contains("agent"));
}

/// A Linux worker that can use sudo is a failure, by group or by `sudo -n -l`.
#[test]
fn worker_user_fails_on_linux_when_the_worker_can_use_sudo() {
    let linux = linux();
    let reason = "the worker account can use sudo; remove it from the sudo group";
    for groups in ["agent sudo", "agent docker wheel", "admin agent"] {
        let out = format!("1001\nos=Linux\ngroups={groups}\nsudo_l=1\n");
        let r = parse(StepId::WORKER_USER, &out, 0, &linux);
        assert_eq!(r.state, CheckState::Fail(reason.into()), "{groups}");
    }
    let by_list = parse(
        StepId::WORKER_USER,
        "1001\nos=Linux\ngroups=agent docker\nsudo_l=0\n",
        0,
        &linux,
    );
    assert_eq!(by_list.state, CheckState::Fail(reason.into()));

    let pass = CheckState::Pass("user id 1001, not root".into());
    let clean = parse(
        StepId::WORKER_USER,
        "1001\nos=Linux\ngroups=agent docker\nsudo_l=1\n",
        0,
        &linux,
    );
    assert_eq!(clean.state, pass);
    // a group that only contains the word is not the group
    let near = parse(
        StepId::WORKER_USER,
        "1001\nos=Linux\ngroups=agent sudoku administrators\nsudo_l=1\n",
        0,
        &linux,
    );
    assert_eq!(near.state, pass);
    // no sudo answer at all (sudo missing) is not a sudoer
    let none = parse(
        StepId::WORKER_USER,
        "1001\nos=Linux\ngroups=agent\n",
        0,
        &linux,
    );
    assert_eq!(none.state, pass);
}

/// On a Mac the user's own login account is in `admin` and is the admin; the sudo test is Linux's.
#[test]
fn worker_user_on_a_mac_may_be_in_the_admin_group() {
    let r = check(
        StepId::WORKER_USER,
        "501\nos=Darwin\ngroups=staff admin everyone\n",
    );
    assert_eq!(r.state, CheckState::Pass("user id 501, not root".into()));
}

#[test]
fn worker_user_probe_asks_the_groups_and_sudo_on_linux_only() {
    let s = step(StepId::WORKER_USER, &mac());
    let text = script(&s);
    assert!(text.starts_with("id -u"), "{text}");
    assert!(text.contains("id -nG"));
    assert!(text.contains("sudo -n -l"));
    assert!(text.contains("Linux"), "the sudo test is guarded for Linux");
    assert!(text.contains("uname -s"));
    assert!(!text.contains("sudo -S"));
    assert!(how_to(&s).contains("sudo"));
}

/// `sudo -n -l` can write a line to the sudo auth log. The poller refreshes every 60 s, so the probe
/// that holds it is in the slow set (on click and about every 10 minutes), never in the cheap batch.
#[test]
fn the_sudo_check_runs_only_in_the_slow_set_never_on_every_refresh() {
    use super::step::Cadence;
    let mut seen = 0;
    for machine in [linux(), mac()] {
        for s in all(&machine) {
            if !script(&s).contains("sudo -n") {
                continue;
            }
            seen += 1;
            assert_eq!(
                s.cadence,
                Cadence::Slow,
                "{} runs sudo -n on every refresh",
                s.id.raw()
            );
        }
    }
    assert!(seen >= 2, "the Worker user probe was looked at");
    assert_eq!(step(StepId::WORKER_USER, &linux()).cadence, Cadence::Slow);
}

/// Root's id wins over the sudo test.
#[test]
fn worker_user_root_still_fails_first() {
    let r = parse(
        StepId::WORKER_USER,
        "0\nos=Linux\ngroups=root\nsudo_l=0\n",
        0,
        &linux(),
    );
    let CheckState::Fail(text) = &r.state else {
        panic!("root should fail");
    };
    assert!(text.contains("root"), "{text}");
}

// MARK: Stays awake

#[test]
fn stays_awake_needs_sleep_disksleep_zero_and_autorestart_one() {
    let bad = check(
        StepId::STAYS_AWAKE,
        " sleep 10\n disksleep 10\n autorestart 0\n",
    );
    assert_eq!(
        bad.state,
        CheckState::Fail(
            "sleep 10 (should be 0), disksleep 10 (should be 0), autorestart 0 (should be 1)"
                .into()
        )
    );
    let good = check(StepId::STAYS_AWAKE, "System-wide power settings:\n SleepDisabled 0\nCurrent Power Settings:\n sleep 0 (sleep prevented by powerd)\n disksleep 0\n displaysleep 10\n autorestart 1\n");
    assert_eq!(
        good.state,
        CheckState::Pass("sleep 0, disksleep 0, autorestart 1".into())
    );
    let partial = check(
        StepId::STAYS_AWAKE,
        " sleep 0\n disksleep 0\n autorestart 0\n",
    );
    assert_eq!(
        partial.state,
        CheckState::Fail("autorestart 0 (should be 1)".into())
    );
    assert!(matches!(
        check(StepId::STAYS_AWAKE, "").state,
        CheckState::CantTell(_)
    ));
    assert_eq!(script(&step(StepId::STAYS_AWAKE, &mac())), "pmset -g");
}

#[test]
fn stays_awake_fix_is_an_admin_pmset() {
    let runner = Arc::new(ScriptedRunner::new());
    let ctx = Ctx::mac().runner(&runner);
    let p = plan(&step(StepId::STAYS_AWAKE, &ctx.machine));
    assert!(p.needs_admin);
    assert!(p
        .display
        .contains("pmset -a sleep 0 disksleep 0 && pmset -a autorestart 1"));
    let outcome = run_fix(&p, &ctx, Some("pw-1234"));
    assert!(outcome.ok, "{}", outcome.message);
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    let sent = call.argv.last().unwrap();
    assert!(sent.contains("pmset -a sleep 0 disksleep 0 && pmset -a autorestart 1"));
    assert!(sent.contains("sudo -S"));
    assert_eq!(call.stdin_text(), "pw-1234\n");
    assert!(!call.argv.iter().any(|a| a.contains("pw-1234")));
    assert!(call.env.iter().all(|(_, v)| !v.contains("pw-1234")));
}

#[test]
fn stays_awake_fix_without_a_password_runs_nothing() {
    let runner = Arc::new(ScriptedRunner::new());
    let ctx = Ctx::mac().runner(&runner);
    let outcome = run_fix(&plan(&step(StepId::STAYS_AWAKE, &ctx.machine)), &ctx, None);
    assert!(!outcome.ok);
    assert!(runner.calls().is_empty());
}

#[test]
fn a_failed_fix_shows_the_last_stderr_line_not_stdout() {
    let runner = failing_runner(
        1,
        "==> Downloading\nprogress 40%\n",
        "warning: x\nError: no formula named foo\n",
    );
    let ctx = Ctx::mac().runner(&runner);
    let outcome = run_fix(&plan(&step(StepId::BASE_TOOLS, &ctx.machine)), &ctx, None);
    assert_eq!(
        outcome,
        FixOutcome::fail("failed (code 1): Error: no formula named foo")
    );
    let quiet = failing_runner(1, "only stdout\n", "");
    let ctx = Ctx::mac().runner(&quiet);
    let from_out = run_fix(&plan(&step(StepId::BASE_TOOLS, &ctx.machine)), &ctx, None);
    assert_eq!(from_out.message, "failed (code 1): only stdout");
}

#[test]
fn the_password_is_replaced_before_the_message_is_cut() {
    let password = format!("pw-{}", "k".repeat(40));
    // the password would straddle character 200 of the line
    let line = format!("{} {password}", "a".repeat(180));
    let runner = failing_runner(1, "", &format!("{line}\n"));
    let ctx = Ctx::mac().runner(&runner);
    let outcome = run_fix(
        &plan(&step(StepId::STAYS_AWAKE, &ctx.machine)),
        &ctx,
        Some(&password),
    );
    assert!(!outcome.message.contains("pw-"), "{}", outcome.message);
    assert!(
        outcome.message.contains("[admin password]"),
        "{}",
        outcome.message
    );
}

#[test]
fn a_failed_fix_message_never_holds_the_admin_password() {
    let runner = failing_runner(1, "", "Sorry, try again pw-1234\n");
    let ctx = Ctx::mac().runner(&runner);
    let outcome = run_fix(
        &plan(&step(StepId::STAYS_AWAKE, &ctx.machine)),
        &ctx,
        Some("pw-1234"),
    );
    assert!(!outcome.ok);
    assert!(!outcome.message.contains("pw-1234"), "{}", outcome.message);
}

// MARK: FileVault

#[test]
fn file_vault_on_and_off() {
    let on = check(StepId::FILEVAULT, "FileVault is On.\n");
    assert_eq!(
        on.state,
        CheckState::Pass("on: after a restart it waits for an unlock".into())
    );
    assert_eq!(on.detected.file_vault, Some(true));
    let off = check(StepId::FILEVAULT, "FileVault is Off.\n");
    assert_eq!(off.state, CheckState::Pass("off".into()));
    assert_eq!(off.detected.file_vault, Some(false));
    let other = parse(
        StepId::FILEVAULT,
        "fdesetup: command not found\n",
        127,
        &mac(),
    );
    assert!(matches!(other.state, CheckState::CantTell(_)));
    assert_eq!(other.detected.file_vault, None);
    assert_eq!(script(&step(StepId::FILEVAULT, &mac())), "fdesetup status");
    assert!(how_to(&step(StepId::FILEVAULT, &mac())).contains("FileVault"));
}
