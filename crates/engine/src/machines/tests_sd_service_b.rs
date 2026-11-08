//! The Service installed step: its Check and the Fix while Running is on (Swift `MachinesSDWorkerStepsTests`).

use super::fakes_b::*;
use super::registry::AdminAccess;
use super::runner::CommandResult;
use super::step::{CheckResult, CheckState, FixOutcome, MacFacts, StepId};
use super::steps_sd::{self};
use super::tests_sd_service::*;

fn mac_check(digest_matches: bool, extra: &str) -> CheckResult {
    let tmp = TempDir::new("maccheck");
    let rig = rig_with_templates(mac("m1"), tmp);
    let files = render_mac();
    let d = digest(if digest_matches {
        &files[0].content
    } else {
        "other"
    });
    service_check(&rig, &format!("{d}  /x/com.sd.worker.plist\n{extra}"))
}

fn line(state: &str) -> String {
    format!("\t\"com.sd.worker\" => {state}\n")
}

#[test]
fn mac_service_check_fails_when_installed_but_not_disabled() {
    let probe = remote(&step_of(&mac("m1"), &StepId::SERVICE)).0;
    assert!(
        probe.contains(r#"launchctl print-disabled "gui/$(id -u)""#),
        "{probe}"
    );
    assert!(
        probe.contains(r#"launchctl print "gui/$(id -u)/com.sd.worker""#),
        "{probe}"
    );

    // plist present, label not disabled (unlisted or enabled), not loaded: a login would start it
    for extra in [
        "notloaded\n".to_string(),
        format!("notloaded\n{}", line("enabled")),
    ] {
        assert_eq!(
            mac_check(true, &extra).state,
            CheckState::Fail("installed but not disabled; a login would start it".into()),
            "{extra}"
        );
    }
    // plist present and disabled: pass, loaded or not
    for extra in [
        format!("notloaded\n{}", line("disabled")),
        format!("notloaded\n{}", line("true")),
        format!("loaded\n{}", line("disabled")),
        // Running is on: enabled and loaded
        "loaded\n".to_string(),
        format!("loaded\n{}", line("enabled")),
    ] {
        assert_eq!(
            mac_check(true, &extra).state,
            CheckState::Pass("com.sd.worker.plist matches".into()),
            "{extra}"
        );
    }
    // a plist that differs still says so first
    assert_eq!(
        mac_check(false, "notloaded\n").state,
        CheckState::Fail("differs: com.sd.worker.plist".into())
    );
}

#[test]
fn service_probe_compares_the_digests_of_the_rendered_units() {
    let tmp = TempDir::new("linuxcheck");
    let rig = rig_with_templates(linux("vps-sing-1"), tmp);
    let probe = remote(&step_of(&rig.machine, &StepId::SERVICE)).0;
    assert_eq!(probe.matches("sha256sum").count(), 4);
    assert!(probe.contains("missing sd-worker.slice"));

    let files = render_linux("agent", "/home/agent").unwrap();
    let good: Vec<String> = files
        .iter()
        .map(|f| format!("{}  {}", digest(&f.content), f.remote_path))
        .collect();
    assert_eq!(
        service_check(&rig, &good.join("\n")).state,
        CheckState::Pass("4 units match".into())
    );

    let mut lines = good.clone();
    lines[1] = format!("{}  /etc/systemd/system/x", "0".repeat(64));
    lines[2] = "missing sd-testflight.service".into();
    assert_eq!(
        service_check(&rig, &lines.join("\n")).state,
        CheckState::Fail(format!(
            "differs: {}; missing: {}",
            files[1].name, files[2].name
        ))
    );

    let mac_probe = remote(&step_of(&mac("m1"), &StepId::SERVICE)).0;
    assert!(mac_probe.contains("shasum -a 256"));
    assert!(!mac_probe.contains("sha256sum"));
}

#[test]
fn service_check_without_this_macs_sd_or_a_known_home_says_why() {
    let rig = Rig::new(linux("vps-sing-1")).with_mac(MacFacts::default());
    assert!(matches!(service_check(&rig, "").state, CheckState::Fail(_)));
    let tmp = TempDir::new("nohome");
    let mut vps = linux("vps-sing-1");
    vps.abilities = None;
    let rig = rig_with_templates(vps, tmp);
    assert_eq!(
        service_check(&rig, "").state,
        CheckState::Fail("the machine's home folder isn't known yet".into())
    );
}

#[test]
fn a_hostile_home_never_reaches_a_unit_a_script_or_a_command() {
    let tmp = TempDir::new("hostile");
    let mut vps = linux("vps-sing-1");
    vps.abilities.as_mut().unwrap().home = Some("/home/a b;x".into());
    let rig = rig_with_templates(vps, tmp);
    assert_eq!(
        service_check(&rig, "").state,
        CheckState::Fail("the home folder isn't a safe path".into())
    );
    let outcome = run_service(&rig);
    assert!(!outcome.ok);
    assert!(rig.calls().is_empty(), "no command was built from it");
    for step in steps_sd::all(
        &rig.machine,
        rig.machine.sd_worker().unwrap(),
        std::slice::from_ref(&rig.machine),
    ) {
        if let Some(script) = step.probe.script() {
            assert!(!script.contains(";x"), "{}", step.id.raw());
        }
        if let Some(p) = step.plan() {
            assert!(!p.display.contains(";x"), "{}", step.id.raw());
        }
    }
}

/// A write failure is masked before it is cut to 300 characters (the units, the LaunchAgent,
/// worker.json's read).
#[test]
fn fix_failures_are_masked_before_they_are_cut() {
    let token = long_token();
    let marker = "[secret:claude-worker]";
    let said = format!("{}{token}", "f".repeat(270));

    // the LaunchAgent write (the disable before it succeeds)
    let tmp = TempDir::new("maskmac");
    let rig = rig_with_templates(mac("m1"), tmp).hiding("claude-worker", &token);
    rig.runner
        .on(script_has("LaunchAgents"), CommandResult::new(1, "", &said));
    let written = run_service(&rig);
    assert!(!written.ok);
    assert_no_fragment(&token, &written.message);
    assert!(written.message.contains(marker), "{}", written.message);

    // the Linux units
    let tmp = TempDir::new("masklinux");
    let mut vps = linux("vps-sing-1");
    vps.set_admin(AdminAccess::RootSsh);
    let rig = rig_with_templates(vps, tmp).hiding("claude-worker", &token);
    rig.runner.on(|_| true, CommandResult::new(1, "", &said));
    let units = run_service(&rig);
    assert!(!units.ok);
    assert_no_fragment(&token, &units.message);

    // worker.json: a read error shows the first line of stderr
    let rig = Rig::new(mac("m1")).hiding("claude-worker", &token);
    rig.runner.on(|_| true, CommandResult::new(1, "", &said));
    let json = (plan(&step_of(&rig.machine, &StepId::WORKER_JSON)).run)(&rig.fix());
    assert!(!json.ok);
    assert_no_fragment(&token, &json.message);
}

// MARK: Service Fix while Running is on

fn mac_service_fix(probe: CommandResult) -> (FixOutcome, Rig) {
    let tmp = TempDir::new("running-on");
    let rig = rig_with_templates(mac("m1"), tmp);
    // the Running probe: `loaded`/`notloaded`, then the label's `print-disabled` line
    rig.runner.on(script_has("print-disabled"), probe);
    let outcome = run_service(&rig);
    (outcome, rig)
}

fn count(rig: &Rig, needle: &str) -> usize {
    rig.argvs()
        .iter()
        .filter(|a| last(a).contains(needle))
        .count()
}

/// With the label loaded and enabled, Running is on: the Fix rewrites the plist and leaves the
/// label enabled.
#[test]
fn service_fix_leaves_the_label_enabled_while_running_is_on() {
    let (outcome, rig) = mac_service_fix(CommandResult::new(1, "loaded\n", ""));
    assert!(outcome.ok, "{}", outcome.message);
    assert_eq!(
        count(&rig, "launchctl disable"),
        0,
        "Running would turn off"
    );
    assert_eq!(count(&rig, "launchctl bootout"), 0);
    let calls = rig.calls();
    let write = calls.last().unwrap();
    assert!(last(&write.argv).contains("com.sd.worker.plist"));
    assert!(write.stdin.is_some(), "the plist is written");
    assert_eq!(calls.len(), 2, "the state probe, then the write");
}

/// A loaded label that `print-disabled` lists as enabled is Running on too.
#[test]
fn service_fix_leaves_a_listed_enabled_label_enabled() {
    let (outcome, rig) = mac_service_fix(CommandResult::new(
        0,
        &format!("loaded\n{}", line("enabled")),
        "",
    ));
    assert!(outcome.ok, "{}", outcome.message);
    assert_eq!(
        count(&rig, "launchctl disable"),
        0,
        "Running would turn off"
    );
    assert_eq!(rig.calls().len(), 2, "the state probe, then the write");
    assert!(
        outcome.message.contains("Running stays on"),
        "{}",
        outcome.message
    );
}

/// Anything but loaded-and-enabled disables the label before the plist is written.
#[test]
fn service_fix_disables_first_unless_running_is_on() {
    let disabled = line("disabled");
    let states = [
        (
            "not loaded, unlisted",
            CommandResult::new(1, "notloaded\n", ""),
        ),
        (
            "not loaded, enabled",
            CommandResult::new(0, &format!("notloaded\n{}", line("enabled")), ""),
        ),
        (
            "loaded but disabled",
            CommandResult::new(0, &format!("loaded\n{disabled}"), ""),
        ),
        (
            "the probe failed",
            CommandResult::new(255, "", "ssh: connect timed out"),
        ),
        ("nothing printed", CommandResult::new(0, "", "")),
    ];
    for (name, probe) in states {
        let (outcome, rig) = mac_service_fix(probe);
        assert!(outcome.ok, "{name}: {}", outcome.message);
        let order: Vec<&str> = rig
            .argvs()
            .iter()
            .map(|a| {
                let s = last(a);
                if s.contains("launchctl disable") {
                    "disable"
                } else if s.contains("com.sd.worker.plist") {
                    "write"
                } else if s.contains("print-disabled") {
                    "probe"
                } else {
                    "other"
                }
            })
            .collect();
        assert_eq!(order, ["probe", "disable", "write"], "{name}");
    }
}
