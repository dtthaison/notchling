use super::fakes::*;
use super::process::CUT_NOTICE;
use super::registry::AdminAccess;
use super::runner::{CommandResult, ScriptedRunner};
use super::shell::PRODUCTION_SERVICES;
use super::step::{CheckState, FixOutcome, StepFix, StepId};
use super::steps_machine::{all, CLAUDE_INSTALL};
use super::tests_machine_steps::{
    check, failing_runner, how_to, linux, mac, parse, parse_in, plan, run_fix, script, step,
};
use std::sync::Arc;

const GOOD_TOOLS: &str = "git=/opt/homebrew/bin/git\npython3=/opt/homebrew/bin/python3\njq=/opt/homebrew/bin/jq\ngh=/opt/homebrew/bin/gh\npyver=3.12\nbrew=yes\n";

#[test]
fn base_tools_needs_python_310_and_finds_its_path() {
    let bad = check(StepId::BASE_TOOLS, "git=/usr/bin/git\npython3=/usr/bin/python3\njq=\ngh=/opt/homebrew/bin/gh\npyver=3.9\nbrew=yes\n");
    assert_eq!(
        bad.state,
        CheckState::Fail("missing: jq; python3 is 3.9, sd needs 3.10 or newer".into())
    );
    let good = check(StepId::BASE_TOOLS, GOOD_TOOLS);
    assert!(good.state.is_pass(), "{:?}", good.state);
    assert_eq!(
        good.detected.python.as_deref(),
        Some("/opt/homebrew/bin/python3")
    );
    assert_eq!(good.rows.len(), 4);
    let ten = check(StepId::BASE_TOOLS, &GOOD_TOOLS.replace("3.12", "3.10"));
    assert!(ten.state.is_pass(), "3.10 is enough");
    let ancient = check(StepId::BASE_TOOLS, &GOOD_TOOLS.replace("3.12", "2.7"));
    assert_eq!(
        ancient.state,
        CheckState::Fail("python3 is 2.7, sd needs 3.10 or newer".into())
    );
    let nothing = check(
        StepId::BASE_TOOLS,
        "git=\npython3=\njq=\ngh=\npyver=\nbrew=no\n",
    );
    assert_eq!(
        nothing.state,
        CheckState::Fail("missing: git, python3, jq, gh".into())
    );
    assert_eq!(nothing.detected.python, None);
}

#[test]
fn base_tools_ignores_an_unsafe_python_path() {
    for path in [
        "/tmp/x y/python3",
        "/tmp/$(id)/python3",
        "relative/python3",
        "/tmp/a;b/python3",
        "/tmp/a`id`/python3",
    ] {
        let r = check(
            StepId::BASE_TOOLS,
            &GOOD_TOOLS.replace("/opt/homebrew/bin/python3", path),
        );
        assert_eq!(r.detected.python, None, "{path}");
    }
}

#[test]
fn base_tools_probe_is_read_only() {
    let s = script(&step(StepId::BASE_TOOLS, &mac()));
    for key in ["git=", "python3=", "jq=", "gh=", "pyver=", "brew="] {
        assert!(s.contains(key), "{key}");
    }
    for word in ["sudo", "install ", "rm "] {
        assert!(!s.contains(word), "{word}");
    }
}

#[test]
fn linux_base_tools_fix_keeps_production_untouched() {
    let mut machine = linux();
    machine.set_admin(AdminAccess::RootSsh);
    let runner = Arc::new(ScriptedRunner::new());
    let ctx = Ctx::new(machine.clone()).runner(&runner);
    let p = plan(&step(StepId::BASE_TOOLS, &machine));
    assert!(p.needs_admin);
    let outcome = run_fix(&p, &ctx, None);
    assert!(outcome.ok, "{}", outcome.message);
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert!(
        call.stdin.is_none(),
        "as root over ssh there is no password"
    );
    assert!(call.argv.contains(&"root@vps-sing-1".to_string()));
    let sent = call.argv.last().unwrap();
    assert!(!sent.contains("sudo"));
    assert!(sent.contains("NEEDRESTART_SUSPEND=1"));
    assert!(sent.contains("apt-get install -y git python3 jq wget"));
    assert!(sent.contains("apt-get install -y gh"));
    assert_eq!(
        sent.matches("--no-upgrade").count(),
        2,
        "both installs keep installed packages as they are"
    );
    let without_flag = sent.replace("--no-upgrade", "");
    for word in ["restart", "upgrade", "systemctl"] {
        assert!(!without_flag.contains(word), "{word}");
    }
    for service in PRODUCTION_SERVICES {
        assert!(!sent.contains(service), "{service}");
    }
    let shown = p.display.replace("--no-upgrade", "");
    for word in ["restart", "upgrade", "systemctl"] {
        assert!(!shown.contains(word), "{word}");
    }
}

#[test]
fn linux_base_tools_fix_as_the_worker_is_refused_before_any_command_runs() {
    let machine = linux();
    for password in [None, Some("pw-1234")] {
        let runner = Arc::new(ScriptedRunner::new());
        let ctx = Ctx::new(machine.clone()).runner(&runner);
        let outcome = run_fix(&plan(&step(StepId::BASE_TOOLS, &machine)), &ctx, password);
        assert!(!outcome.ok);
        assert!(
            outcome
                .message
                .contains("On Linux the worker account can't be the admin"),
            "{}",
            outcome.message
        );
        assert!(runner.calls().is_empty());
    }
}

#[test]
fn mac_base_tools_fix_uses_homebrew_without_admin() {
    let runner = Arc::new(ScriptedRunner::new());
    let ctx = Ctx::mac().runner(&runner);
    let p = plan(&step(StepId::BASE_TOOLS, &ctx.machine));
    assert!(!p.needs_admin);
    let outcome = run_fix(&p, &ctx, None);
    assert!(outcome.ok, "{}", outcome.message);
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].stdin.is_none());
    let sent = calls[0].argv.last().unwrap();
    assert!(!sent.contains("sudo"));
    assert!(sent.contains("brew install git python jq gh"));
    assert!(sent.contains("Homebrew is missing: install it from https://brew.sh"));
    assert!(p.display.contains("brew install git python jq gh"));
    assert!(
        sent.contains("xcode-select -p"),
        "Homebrew needs the Command Line Tools"
    );
}

#[test]
fn base_tools_fix_refuses_when_the_os_is_not_detected_yet() {
    let mut machine = mac();
    machine.os = None;
    let runner = Arc::new(ScriptedRunner::new());
    let ctx = Ctx::new(machine.clone()).runner(&runner);
    let p = plan(&step(StepId::BASE_TOOLS, &machine));
    assert!(!p.needs_admin);
    let outcome = run_fix(&p, &ctx, None);
    assert_eq!(
        outcome,
        FixOutcome::fail("OS not detected yet; run Reachable first")
    );
    assert!(runner.calls().is_empty());
}

#[test]
fn a_fix_built_for_one_os_refuses_after_the_machines_os_changed() {
    let built = step(StepId::BASE_TOOLS, &mac());
    let runner = Arc::new(ScriptedRunner::new());
    let ctx = Ctx::new(linux()).runner(&runner);
    let outcome = run_fix(&plan(&built), &ctx, None);
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("OS changed"),
        "{}",
        outcome.message
    );
    assert!(runner.calls().is_empty());
}

#[test]
fn base_tools_without_command_line_tools_fails_without_running_python() {
    let r = check(
        StepId::BASE_TOOLS,
        "clt=missing\ngit=/usr/bin/git\npython3=/usr/bin/python3\njq=\ngh=\npyver=\nbrew=no\n",
    );
    assert_eq!(
        r.state,
        CheckState::Fail("Command Line Tools missing".into())
    );
    assert_eq!(r.detected.python, None);
    let s = script(&step(StepId::BASE_TOOLS, &mac()));
    let guard_at = s.find("xcode-select -p >/dev/null 2>&1").unwrap();
    let python_at = s.find("python3 -c").unwrap();
    assert!(guard_at < python_at);
    assert!(s.contains("Darwin"));
    assert!(!s.contains("git --version"));
}

#[test]
fn mac_base_tools_fix_reports_a_missing_homebrew() {
    let runner = failing_runner(1, "Homebrew is missing: install it from https://brew.sh in the machine's own Terminal, then Check again.\n", "");
    let ctx = Ctx::mac().runner(&runner);
    let outcome = run_fix(&plan(&step(StepId::BASE_TOOLS, &ctx.machine)), &ctx, None);
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("Homebrew is missing"),
        "{}",
        outcome.message
    );
}

#[test]
fn claude_code_shows_its_version() {
    let ok = check(StepId::CLAUDE_CODE, "2.1.330 (Claude Code)\n");
    assert_eq!(ok.state, CheckState::Pass("2.1.330 (Claude Code)".into()));
    let none = check(StepId::CLAUDE_CODE, "");
    assert_eq!(none.state, CheckState::Fail("not installed".into()));
    let missing = parse(StepId::CLAUDE_CODE, "", 1, &mac());
    assert_eq!(missing.state, CheckState::Fail("not installed".into()));
}

#[test]
fn machine_text_is_masked_before_it_is_cut_in_checks_and_fixes() {
    let token = long_token();
    let marker = "[secret:claude-worker]";
    let ctx = Ctx::mac().hiding("claude-worker", &token);

    let version = parse_in(
        StepId::CLAUDE_CODE,
        &format!("2.1.0 {}{token}\n", "f".repeat(164)),
        0,
        &ctx,
    );
    assert_no_fragment(&token, &format!("{:?}", version.state));
    assert!(format!("{:?}", version.state).contains(marker));
    let reach = parse_in(
        StepId::REACHABLE,
        &format!("{}{token}\n", "f".repeat(160)),
        255,
        &ctx,
    );
    assert_no_fragment(&token, &format!("{:?}", reach.state));
    let vault = parse_in(
        StepId::FILEVAULT,
        &format!("{}{token}\n", "f".repeat(170)),
        0,
        &ctx,
    );
    assert_no_fragment(&token, &format!("{:?}", vault.state));

    let said = format!("{}{token}", "f".repeat(170));
    let runner = failing_runner(1, "", &said);
    let fix_ctx = Ctx::mac().runner(&runner).hiding("claude-worker", &token);
    let fix = run_fix(
        &plan(&step(StepId::CLAUDE_CODE, &fix_ctx.machine)),
        &fix_ctx,
        None,
    );
    assert!(!fix.ok);
    assert_no_fragment(&token, &fix.message);
    assert!(fix.message.contains(marker), "{}", fix.message);

    let suffix: String = token.chars().skip(token.chars().count() - 60).collect();
    let cut_runner = failing_runner(1, "", &format!("{CUT_NOTICE}{suffix}"));
    let cut_ctx = Ctx::mac()
        .runner(&cut_runner)
        .hiding("claude-worker", &token);
    let cut = run_fix(
        &plan(&step(StepId::CLAUDE_CODE, &cut_ctx.machine)),
        &cut_ctx,
        None,
    );
    assert_no_fragment(&token, &cut.message);
}

#[test]
fn tool_paths_and_xcode_names_are_masked_before_they_are_cut() {
    let token = long_token();
    let marker = "[secret:claude-worker]";
    let ctx = Ctx::mac().hiding("claude-worker", &token);
    let filler = "f".repeat(150);

    let tools = parse_in(
        StepId::BASE_TOOLS,
        &format!("git=/usr/bin/{filler}{token}\npython3=/usr/bin/python3\njq=/usr/bin/jq\ngh=/usr/bin/gh\npyver=3.12\nbrew=yes\n"),
        0,
        &ctx,
    );
    let git_row = tools.rows.iter().find(|r| r.name == "git").unwrap();
    assert_no_fragment(&token, &git_row.detail);
    assert!(git_row.detail.contains(marker), "{}", git_row.detail);

    let devices = format!("{{\"devices\":{{\"com.apple.CoreSimulator.SimRuntime.iOS-26-5\":[{{\"name\":\"iPhone {filler}{token}\",\"isAvailable\":true}}]}}}}");
    let xcode = parse_in(
        StepId::XCODE,
        &format!("Xcode 26.1 {filler}{token}\n@@runtimes\niOS {filler}{token} (26.5 - 23F77) - x\n@@devices\n{devices}\n"),
        0,
        &ctx,
    );
    let a = xcode.detected.abilities.clone().unwrap();
    let xcode_line = a.xcode.clone().unwrap();
    for shown in [
        xcode_line.clone(),
        a.ios_runtimes.join(""),
        a.simulators.join(""),
        format!("{:?}", xcode.state),
    ] {
        assert_no_fragment(&token, &shown);
    }
    assert!(xcode_line.contains(marker));
    assert!(a.ios_runtimes[0].contains(marker), "{:?}", a.ios_runtimes);
    assert!(a.simulators[0].contains(marker), "{:?}", a.simulators);
}

#[test]
fn a_fix_failure_hides_the_admin_password_through_the_mask() {
    let runner = failing_runner(1, "", "sudo: wrong password hunter22 for agent");
    let ctx = Ctx::mac().runner(&runner);
    let hidden = run_fix(
        &plan(&step(StepId::CLAUDE_CODE, &ctx.machine)),
        &ctx,
        Some("hunter22"),
    );
    assert_eq!(
        hidden.message,
        "failed (code 1): sudo: wrong password [admin password] for agent"
    );
}

#[test]
fn claude_code_fix_installs_as_the_worker_without_admin() {
    let p = plan(&step(StepId::CLAUDE_CODE, &mac()));
    assert!(!p.needs_admin);
    assert!(p.display.contains(CLAUDE_INSTALL));
    let runner = Arc::new(ScriptedRunner::new());
    let ctx = Ctx::mac().runner(&runner);
    let outcome = run_fix(&p, &ctx, None);
    assert!(outcome.ok, "{}", outcome.message);
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0]
        .argv
        .last()
        .unwrap()
        .contains("curl -fsSL https://claude.ai/install.sh"));
    assert!(!calls[0].argv.last().unwrap().contains("sudo"));
    runner.on(|_| true, CommandResult::new(22, "", "curl: (22) 404"));
    assert!(!run_fix(&p, &ctx, None).ok);
}

#[cfg(unix)]
fn run_install(curl: &str) -> i32 {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("curl");
    let fake = dir.join("curl");
    std::fs::write(&fake, format!("#!/bin/sh\n{curl}\n")).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let status = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(CLAUDE_INSTALL)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    status.code().unwrap_or(-1)
}

#[cfg(unix)]
#[test]
fn claude_install_script_fails_when_the_download_fails() {
    assert_eq!(run_install("exit 22"), 22);
    assert_ne!(run_install("exit 6"), 0);
    assert_eq!(run_install("echo 'exit 0'"), 0);
    assert_eq!(run_install("echo 'exit 3'"), 3);
}

#[test]
fn xcode_needs_a_runtime_and_an_iphone() {
    let text = SIMCTL_OK;
    let ok = check(StepId::XCODE, text);
    assert_eq!(
        ok.state,
        CheckState::Pass("Xcode 26.1 · iOS 26.5 · iPhone 17 and 2 more".into())
    );
    let a = ok.detected.abilities.unwrap();
    assert_eq!(a.xcode.as_deref(), Some("Xcode 26.1"));
    assert_eq!(a.ios_runtimes, vec!["iOS 26.5"]);
    assert_eq!(
        a.simulators,
        vec!["iPhone 17", "iPhone 17 Pro", "iPhone 17 Pro Max"]
    );

    let no_phone = check(
        StepId::XCODE,
        &text.replace("\"name\" : \"iPhone", "\"name\" : \"iPad"),
    );
    assert_eq!(
        no_phone.state,
        CheckState::Fail("no iPhone simulator".into())
    );

    let from_marker = &text[text.find('@').unwrap()..];
    let no_xcode = check(StepId::XCODE, from_marker);
    assert_eq!(
        no_xcode.state,
        CheckState::Fail("Xcode is not installed".into())
    );

    let no_runtime = check(
        StepId::XCODE,
        &text.replace(
            "iOS 26.5 (26.5 - 23F77) - com.apple.CoreSimulator.SimRuntime.iOS-26-5\n",
            "",
        ),
    );
    assert_eq!(no_runtime.state, CheckState::Fail("no iOS runtime".into()));

    let two = check(
        StepId::XCODE,
        &text.replace(
            "iOS 26.5 (26.5 - 23F77)",
            "iOS 26.5 (26.5 - 23F77) - x\niOS 27.0 (27.0 - 24A1)",
        ),
    );
    assert_eq!(
        two.detected.abilities.unwrap().ios_runtimes,
        vec!["iOS 27.0", "iOS 26.5"]
    );

    assert!(how_to(&step(StepId::XCODE, &mac())).contains("Apple ID"));
}

#[test]
fn xcode_probe_asks_for_a_runtime_line_not_runtime_json() {
    let s = script(&step(StepId::XCODE, &mac()));
    assert!(s.contains("xcodebuild -version | head -1"));
    assert!(s.contains("@@runtimes") && s.contains("@@devices"));
    assert!(s.contains("xcrun simctl list runtimes available 2>/dev/null | grep '^iOS '"));
    assert!(
        !s.contains("runtimes available -j"),
        "the runtimes JSON alone is 48 KB"
    );
    assert!(s.contains("xcrun simctl list devices iPhone -j"));
    assert!(
        !s.contains("devices available"),
        "the unfiltered list can pass the 64 KiB cap"
    );
}

#[test]
fn xcode_probe_asks_xcode_select_first_and_says_so_when_the_tools_are_missing() {
    let s = script(&step(StepId::XCODE, &mac()));
    let guard_at = s.find("xcode-select -p >/dev/null 2>&1").unwrap();
    assert!(guard_at < s.find("xcodebuild").unwrap());
    let mut known = mac();
    known.abilities.as_mut().unwrap().xcode = Some("Xcode 25.0".into());
    let r = parse(StepId::XCODE, "@@noclt\n", 0, &known);
    assert_eq!(
        r.state,
        CheckState::Fail("Xcode is not installed: the Command Line Tools are missing".into())
    );
    assert!(
        r.detected.abilities.is_none(),
        "the saved abilities stay as they are"
    );
}

#[test]
fn xcode_cant_tell_when_the_output_was_cut_or_cannot_be_read() {
    let text = SIMCTL_OK;
    let mut known = mac();
    {
        let a = known.abilities.as_mut().unwrap();
        a.xcode = Some("Xcode 25.0".into());
        a.simulators = vec!["iPhone 16".into()];
    }
    let cut_start = format!("[earlier output cut]\n{}", &text[40..]);
    let cut_end = text[..text.len() - 120].to_string();
    let broken = "Xcode 26.1\n@@runtimes\niOS 26.5 (26.5 - 23F77) - x\n@@devices\n{ not json\n";
    let at_cap = format!("{text}{}", " ".repeat(65_536));
    let no_marker = "Xcode 26.1\n";
    for (name, input) in [
        ("cut start", cut_start.as_str()),
        ("cut end", cut_end.as_str()),
        ("broken", broken),
        ("cap", at_cap.as_str()),
        ("no marker", no_marker),
    ] {
        let r = parse(StepId::XCODE, input, 0, &known);
        assert!(
            matches!(r.state, CheckState::CantTell(_)),
            "{name} should be can't tell, got {:?}",
            r.state
        );
        assert!(
            r.detected.abilities.is_none(),
            "{name} must not overwrite the abilities"
        );
    }
    let odd = parse(
        StepId::XCODE,
        "Xcode 26.1\n@@runtimes\niOS ??\n@@devices\n{\"devices\":{}}\n",
        0,
        &known,
    );
    assert!(matches!(odd.state, CheckState::CantTell(_)));
}

#[test]
fn xcode_fixture_has_three_iphones() {
    assert_eq!(SIMCTL_OK.matches("\"name\" : \"iPhone").count(), 3);
}

#[test]
fn no_fix_command_holds_text_the_machine_reported() {
    let hostile = "/home/a b;x$(id)";
    for base in [mac(), linux()] {
        let mut machine = base;
        machine.set_admin(AdminAccess::RootSsh);
        machine.abilities.as_mut().unwrap().home = Some(hostile.into());
        machine.abilities.as_mut().unwrap().xcode = Some(hostile.into());
        for s in all(&machine) {
            let StepFix::Run(p) = &s.fix else { continue };
            let runner = Arc::new(ScriptedRunner::new());
            let ctx = Ctx::new(machine.clone()).runner(&runner);
            run_fix(p, &ctx, Some("pw-1234"));
            for call in runner.calls() {
                assert!(
                    !call.argv.join(" ").contains("a b;x"),
                    "{} put machine text in a command",
                    s.id.raw()
                );
            }
        }
    }
}
