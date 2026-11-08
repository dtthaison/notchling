//! Tests of the setup check, its create-labels Fix and the Running switch (Swift
//! `MachinesSDWorkerStepsTests`, last part).

use super::fakes_b::*;
use super::registry::Machine;
use super::runner::CommandResult;
use super::shell;
use super::step::{
    CheckResult, CheckState, FixOutcome, MacFacts, RemoteOutput, SetupStep, StepFix, StepId, SubRow,
};
use super::steps_sd::{self, is_at_least, toggle_script, MIN_SD};

fn step_of(m: &Machine, id: &StepId) -> SetupStep {
    find(
        &steps_sd::all(m, m.sd_worker().unwrap(), std::slice::from_ref(m)),
        id,
    )
}

fn check(id: &StepId, m: &Machine, out: &str) -> CheckResult {
    let rig = Rig::new(m.clone()).with_mac(mac_facts("1.19.2", "/Users/x/sd"));
    let (_, parse) = remote(&step_of(m, id));
    parse(
        &RemoteOutput {
            text: out.into(),
            code: 0,
        },
        &rig.step(),
    )
}

fn script(id: &StepId, m: &Machine) -> String {
    remote(&step_of(m, id)).0
}

fn fix_with(m: &Machine, facts: MacFacts, rig_out: impl FnOnce(&Rig)) -> (FixOutcome, Rig) {
    let rig = Rig::new(m.clone()).with_mac(facts);
    rig_out(&rig);
    let outcome = (plan(&step_of(m, &StepId::SD_CHECK)).run)(&rig.fix());
    (outcome, rig)
}

const CHECK_ROWS: &str = r#"
[{"name":"tools","ok":true,"detail":"claude, git, gh"},
 {"name":"labels","ok":false,"detail":"missing sd:queued"},
 {"name":"token expiry","ok":false,"detail":"9 days left"},
 {"name":"com.sd.worker","ok":false,"detail":"the LaunchAgent is not loaded"}]"#;

// MARK: sd worker setup --check

#[test]
fn setup_check_rows_become_sub_steps() {
    let m1 = mac("m1");
    let failed = check(&StepId::SD_CHECK, &m1, &format!("{CHECK_ROWS}\n@@exit 4\n"));
    assert_eq!(
        failed.state,
        CheckState::Fail("2 of 3 checks failed".into())
    );
    assert_eq!(failed.rows.len(), 4, "the com.sd.worker row is listed");
    assert_eq!(
        failed.rows[0],
        SubRow::new("tools", true, "claude, git, gh")
    );
    assert_eq!(failed.rows[3].name, "com.sd.worker");

    let pass_rows =
        r#"[{"name":"tools","ok":true,"detail":"x"},{"name":"login","ok":true,"detail":"y"}]"#;
    let ok = check(&StepId::SD_CHECK, &m1, &format!("{pass_rows}\n@@exit 0\n"));
    assert_eq!(ok.state, CheckState::Pass("all 2 checks passed".into()));

    // only the Running row fails: exit 4, but nothing the user has to fix here
    let only_running = r#"[{"name":"tools","ok":true,"detail":"x"},{"name":"com.sd.worker","ok":false,"detail":"not loaded"}]"#;
    let idle = check(
        &StepId::SD_CHECK,
        &m1,
        &format!("{only_running}\n@@exit 4\n"),
    );
    assert_eq!(idle.state, CheckState::Pass("all 1 checks passed".into()));

    let broken = check(
        &StepId::SD_CHECK,
        &m1,
        "\n@@exit 2\nconfig: token_file: ~/.config/agents/github-token does not exist\nsecond line\n",
    );
    assert_eq!(
        broken.state,
        CheckState::Fail("config: token_file: ~/.config/agents/github-token does not exist".into())
    );
    assert!(broken.rows.is_empty());

    let empty = check(
        &StepId::SD_CHECK,
        &m1,
        "\n@@exit 3\ntoken_file /x is empty\n",
    );
    assert_eq!(
        empty.state,
        CheckState::Fail("token_file /x is empty".into())
    );

    // exit 4 with every failing row being the uncounted one is still a fail when no row is named
    let none_named = check(
        &StepId::SD_CHECK,
        &m1,
        "[{\"name\":\"tools\",\"ok\":true,\"detail\":\"x\"}]\n@@exit 4\n",
    );
    assert_eq!(
        none_named.state,
        CheckState::Fail("setup --check failed but named no row".into())
    );
}

#[test]
fn setup_check_bounds_what_the_machine_sends() {
    let long = "a".repeat(1000);
    let many: Vec<String> = (0..300)
        .map(|i| format!("{{\"name\":\"r{i}\",\"ok\":true,\"detail\":\"{long}\"}}"))
        .collect();
    let m = mac("m1");
    let result = check(
        &StepId::SD_CHECK,
        &m,
        &format!("[{}]\n@@exit 0\n", many.join(",")),
    );
    assert_eq!(result.rows.len(), 200);
    assert_eq!(result.rows[0].detail.chars().count(), 300);
    assert_eq!(
        result.state,
        CheckState::Pass("all 200 checks passed".into())
    );

    assert!(check(&StepId::SD_CHECK, &m, "not json\n@@exit 0\n")
        .state
        .is_fail());
    assert!(check(&StepId::SD_CHECK, &m, "").state.is_fail());
    assert!(check(&StepId::SD_CHECK, &m, "[1,2]\n@@exit 0\n")
        .state
        .is_fail());
}

/// A row's text and a failure line are masked before they are cut to 300 characters: the cut falls
/// inside the token.
#[test]
fn setup_check_masks_what_the_machine_says_before_it_cuts_it() {
    let token = long_token();
    let marker = "[secret:claude-worker]";
    let m1 = mac("m1");
    let rig = Rig::new(m1.clone())
        .with_mac(mac_facts("1.19.2", "/x"))
        .hiding("claude-worker", &token);
    let (_, parse) = remote(&step_of(&m1, &StepId::SD_CHECK));
    let said = format!("{}{token}", "f".repeat(270));

    let rows = parse(
        &RemoteOutput {
            text: format!(
                "[{{\"name\":\"{said}\",\"ok\":true,\"detail\":\"{said}\"}}]\n@@exit 0\n"
            ),
            code: 0,
        },
        &rig.step(),
    );
    assert_eq!(rows.rows.len(), 1);
    assert_no_fragment(&token, &rows.rows[0].name);
    assert_no_fragment(&token, &rows.rows[0].detail);
    assert!(
        rows.rows[0].detail.contains(marker),
        "{}",
        rows.rows[0].detail
    );

    let broken = parse(
        &RemoteOutput {
            text: format!("\n@@exit 2\n{said}\n"),
            code: 0,
        },
        &rig.step(),
    );
    let CheckState::Fail(text) = &broken.state else {
        panic!("{:?}", broken.state)
    };
    assert_no_fragment(&token, text);
    assert!(text.contains(marker), "{text}");
}

// MARK: sd 1.19.3: the check is read-only, its Fix creates the labels

const LABEL_ROWS: &str = r#"
[{"name":"tools","ok":true,"detail":"claude, git, gh"},
 {"name":"labels dtthaison/lingeva-mono","ok":true,"detail":"created sd:ready, sd:claimed; the token can write issues"},
 {"name":"com.sd.worker","ok":false,"detail":"the LaunchAgent is not loaded"}]"#;

fn mac_1193() -> MacFacts {
    mac_facts(
        "1.19.3",
        "/Users/x/.claude/plugins/cache/system-design/sd/1.19.3",
    )
}

#[test]
fn the_setup_check_keeps_its_read_only_command_and_its_fix_creates_the_labels() {
    let m1 = mac("m1");
    let sd = shell::sd_py("/Users/tester");
    let c = script(&StepId::SD_CHECK, &m1);
    assert!(
        c.contains(&format!(
            "'/opt/homebrew/bin/python3' -B '{sd}' worker setup --check --json"
        )),
        "{c}"
    );
    assert!(
        !c.contains("--create-labels"),
        "the check never creates anything"
    );
    let p = plan(&step_of(&m1, &StepId::SD_CHECK));
    assert!(!p.needs_admin);
    assert!(
        p.display.contains(&format!(
            "'/opt/homebrew/bin/python3' -B '{sd}' worker setup --create-labels --json"
        )),
        "{}",
        p.display
    );
    assert!(!p.display.contains("--check"), "{}", p.display);
    assert!(!p.display.contains("--live"));
}

#[test]
fn the_create_labels_fix_runs_only_that_command_and_reports_the_label_rows() {
    let (out, rig) = fix_with(&mac("m1"), mac_1193(), |rig| {
        rig.runner.on(
            script_has("--create-labels"),
            CommandResult::ok(&format!("{LABEL_ROWS}\n@@exit 4\n")),
        );
    });
    assert_eq!(rig.calls().len(), 1);
    let sent = rig.argvs()[0].last().cloned().unwrap();
    assert!(
        sent.contains("worker setup --create-labels --json"),
        "{sent}"
    );
    assert!(!sent.contains("--live"));
    assert!(out.ok, "{}", out.message);
    assert!(
        out.message.contains("created sd:ready, sd:claimed"),
        "{}",
        out.message
    );
}

#[test]
fn the_create_labels_fix_fails_on_a_label_row_and_asks_for_a_check_when_other_rows_fail() {
    let cannot = r#"[{"name":"labels dtthaison/lingeva-mono","ok":false,"detail":"cannot create labels: HTTP 403"}]"#;
    let (no, _) = fix_with(&mac("m1"), mac_1193(), |r| {
        r.runner.on(
            |_| true,
            CommandResult::ok(&format!("{cannot}\n@@exit 4\n")),
        );
    });
    assert!(!no.ok);
    assert!(
        no.message.contains("cannot create labels: HTTP 403"),
        "{}",
        no.message
    );

    let others = r#"[{"name":"labels dtthaison/lingeva-mono","ok":true,"detail":"6 labels present"},{"name":"token expiry","ok":false,"detail":"9 days left"}]"#;
    let (some, _) = fix_with(&mac("m1"), mac_1193(), |r| {
        r.runner.on(
            |_| true,
            CommandResult::ok(&format!("{others}\n@@exit 4\n")),
        );
    });
    assert!(!some.ok);
    assert!(some.message.contains("Check again"), "{}", some.message);

    let (bad, _) = fix_with(&mac("m1"), mac_1193(), |r| {
        r.runner.on(
            |_| true,
            CommandResult::ok(
                "\n@@exit 2\nsd: worker setup needs --check, --create-labels or both\n",
            ),
        );
    });
    assert!(!bad.ok);
    assert!(
        bad.message.contains("worker setup needs"),
        "{}",
        bad.message
    );

    // the command itself exits non-zero: the first line it said
    let (failed, _) = fix_with(&mac("m1"), mac_1193(), |r| {
        r.runner.on(
            |_| true,
            CommandResult::new(255, "", "ssh: connect timed out\n"),
        );
    });
    assert_eq!(failed.message, "ssh: connect timed out");
}

/// sd 1.19.3 is the first with `--create-labels`: an older sd on this Mac (which is the one copied
/// to the machine) stops the Fix before any command.
#[test]
fn the_create_labels_fix_needs_sd_1_19_3_on_this_mac() {
    let m1 = mac("m1");
    for facts in [mac_facts("1.19.2", "/x"), MacFacts::default()] {
        let (out, rig) = fix_with(&m1, facts, |_| {});
        assert!(!out.ok);
        assert!(rig.calls().is_empty(), "nothing ran");
    }
    let (old, _) = fix_with(&m1, mac_facts("1.19.2", "/x"), |_| {});
    assert_eq!(
        old.message,
        format!("sd on this Mac is older than {MIN_SD}; update it first")
    );
    assert!(is_at_least("1.19.3", "1.19.3"));
    assert!(!is_at_least("1.9.9", "1.19.3"));
}

#[test]
fn the_setup_check_command_uses_absolute_paths_and_never_live() {
    let vps = linux("vps-sing-1");
    let s = script(&StepId::SD_CHECK, &vps);
    let sd = shell::sd_py("/home/agent");
    assert!(
        s.contains(&format!(
            "'/usr/bin/python3' -B '{sd}' worker setup --check --json"
        )),
        "{s}"
    );
    assert!(!s.contains("--live"));
    let step = step_of(&vps, &StepId::SD_CHECK);
    assert!(matches!(step.probe, super::step::Probe::Remote { .. }));
    assert_eq!(step.cadence, super::step::Cadence::Slow);

    let mut young = vps.clone();
    young.sd_worker_mut().unwrap().python = "python3".into();
    let refused = script(&StepId::SD_CHECK, &young);
    assert!(
        !refused.contains("worker setup"),
        "a relative python never runs"
    );
}

#[test]
fn a_python_or_sd_path_from_a_machine_with_shell_characters_never_runs() {
    for (python, sd_path) in [
        ("/usr/bin/python3; touch /tmp/x", "/a/sd.py"),
        ("/usr/bin/python3", "/a b/sd.py"),
        ("/usr/bin/python3", "/a/$(id)/sd.py"),
        ("/usr/bin/py'thon", "/a/sd.py"),
    ] {
        let mut m = linux("vps-sing-1");
        let s = m.sd_worker_mut().unwrap();
        s.python = python.into();
        s.sd_path = sd_path.into();
        let probe = script(&StepId::SD_CHECK, &m);
        assert!(
            !probe.contains("worker setup") && !probe.contains("touch"),
            "{python}: {probe}"
        );
        let p = plan(&step_of(&m, &StepId::SD_CHECK));
        assert!(
            !p.display.contains("touch") && !p.display.contains("$(id)"),
            "{}",
            p.display
        );
        let (out, rig) = fix_with(&m, mac_1193(), |_| {});
        assert!(!out.ok);
        assert!(rig.calls().is_empty(), "{python}");
    }
}

// MARK: Running

#[cfg(unix)]
fn uid() -> String {
    let out = std::process::Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Runs the macOS toggle scripts with `/bin/sh` and a fake `launchctl` that logs its arguments.
#[cfg(unix)]
fn run_toggle(on: bool, loaded: bool) -> Vec<String> {
    let dir = TempDir::new("toggle");
    let bin = dir.path().join("bin");
    let log = dir.path().join("log");
    let fake = format!(
        "echo \"$*\" >> '{}'\nif [ \"$1\" = print ]; then [ {} = 1 ]; exit $?; fi\nexit 0",
        log.display(),
        if loaded { "1" } else { "0" }
    );
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(&bin).unwrap();
        let p = bin.join("launchctl");
        std::fs::write(&p, format!("#!/bin/sh\n{fake}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let status = std::process::Command::new("/bin/sh")
        .args(["-c", &toggle_script(false, on)])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", "/Users/x")
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

#[cfg(unix)]
#[test]
fn mac_toggle_on_enables_then_bootstraps_only_when_not_loaded() {
    let uid = uid();
    assert_eq!(
        run_toggle(true, false),
        [
            format!("enable gui/{uid}/com.sd.worker"),
            format!("print gui/{uid}/com.sd.worker"),
            format!("bootstrap gui/{uid} /Users/x/Library/LaunchAgents/com.sd.worker.plist"),
        ]
    );
    assert_eq!(
        run_toggle(true, true),
        [
            format!("enable gui/{uid}/com.sd.worker"),
            format!("print gui/{uid}/com.sd.worker")
        ],
        "already loaded is ok"
    );
}

#[cfg(unix)]
#[test]
fn mac_toggle_off_boots_out_only_when_loaded_then_disables() {
    let uid = uid();
    assert_eq!(
        run_toggle(false, true),
        [
            format!("print gui/{uid}/com.sd.worker"),
            format!("bootout gui/{uid}/com.sd.worker"),
            format!("disable gui/{uid}/com.sd.worker"),
        ]
    );
    assert_eq!(
        run_toggle(false, false),
        [
            format!("print gui/{uid}/com.sd.worker"),
            format!("disable gui/{uid}/com.sd.worker")
        ],
        "not loaded is ok"
    );
}

#[test]
fn running_toggle_scripts_and_state() {
    assert_eq!(
        toggle_script(true, true),
        "systemctl enable --now sd-worker.service"
    );
    assert_eq!(
        toggle_script(true, false),
        "systemctl disable --now sd-worker.service"
    );
    let on = toggle_script(false, true);
    assert!(on.starts_with(r#"launchctl enable "gui/$(id -u)/com.sd.worker""#));
    assert!(on.ends_with(
        r#"launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.sd.worker.plist""#
    ));
    let off = toggle_script(false, false);
    assert!(off.contains(r#"launchctl bootout "gui/$(id -u)/com.sd.worker""#));
    assert!(off.ends_with(r#"launchctl disable "gui/$(id -u)/com.sd.worker""#));
    for linux in [true, false] {
        for on in [true, false] {
            let s = toggle_script(linux, on);
            assert!(!s.contains("sd-testflight"));
            for service in shell::PRODUCTION_SERVICES {
                assert!(!s.contains(service));
            }
        }
    }

    let vps = linux("vps-sing-1");
    let m1 = mac("m1");
    assert_eq!(
        script(&StepId::RUNNING, &vps),
        "systemctl is-enabled sd-worker.service; systemctl is-active sd-worker.service"
    );
    assert_eq!(
        script(&StepId::RUNNING, &m1),
        r#"launchctl print "gui/$(id -u)/com.sd.worker" >/dev/null 2>&1 && echo loaded || echo notloaded; launchctl print-disabled "gui/$(id -u)" 2>/dev/null | grep '"com.sd.worker" =>'"#
    );
    assert_eq!(
        check(&StepId::RUNNING, &vps, "enabled\nactive\n").state,
        CheckState::Pass("on".into())
    );
    for off in [
        "disabled\ninactive\n",
        "enabled\ninactive\n",
        "disabled\nactive\n",
        "",
        "enabled\nfailed\n",
    ] {
        assert_eq!(
            check(&StepId::RUNNING, &vps, off).state,
            CheckState::Fail("off".into()),
            "{off:?}"
        );
    }
    // macOS: on is loaded and not disabled; a label print-disabled doesn't list counts as enabled
    let line = |state: &str| format!("\t\"com.sd.worker\" => {state}\n");
    assert_eq!(
        check(&StepId::RUNNING, &m1, "loaded\n").state,
        CheckState::Pass("on".into())
    );
    assert_eq!(
        check(
            &StepId::RUNNING,
            &m1,
            &format!("loaded\n{}", line("enabled"))
        )
        .state,
        CheckState::Pass("on".into())
    );
    for off in [
        "notloaded\n".to_string(),
        format!("notloaded\n{}", line("disabled")),
        format!("loaded\n{}", line("disabled")),
        format!("loaded\n{}", line("true")),
        format!("notloaded\n{}", line("enabled")),
        String::new(),
    ] {
        assert_eq!(
            check(&StepId::RUNNING, &m1, &off).state,
            CheckState::Fail("off".into()),
            "{off:?}"
        );
    }

    // the toggle is a click's switch, not a Fix
    assert!(matches!(
        step_of(&vps, &StepId::RUNNING).fix,
        StepFix::Toggle { needs_admin: true }
    ));
    assert!(matches!(
        step_of(&m1, &StepId::RUNNING).fix,
        StepFix::Toggle { needs_admin: false }
    ));
    assert_eq!(
        step_of(&vps, &StepId::RUNNING).needs,
        vec![StepId::REACHABLE]
    );
}
