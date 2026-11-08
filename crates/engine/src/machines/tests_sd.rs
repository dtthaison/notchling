//! Tests of the sd worker job's plugins and worker.json steps (Swift `MachinesSDWorkerStepsTests`,
//! first part). No ssh starts: runners are scripted, and the archive `tar` would make is written by
//! the scripted runner itself.

use super::fakes_b::*;
use super::mac_facts;
use super::registry::Machine;
use super::runner::{CommandResult, Recorded};
use super::step::{CheckResult, CheckState, Detected, MacFacts, RemoteOutput, SetupStep, StepId};
use super::steps_sd::{self, MIN_SD};
use serde_json::json;

pub(super) const SD_LINE: &str =
    "sd=/home/agent/.local/share/agents/system-design/plugin/scripts/sd.py\n";

pub(super) fn steps(m: &Machine, all: Option<Vec<Machine>>) -> Vec<SetupStep> {
    let all = all.unwrap_or_else(|| vec![m.clone()]);
    steps_sd::all(m, m.sd_worker().unwrap(), &all)
}

pub(super) fn default_mac() -> MacFacts {
    mac_facts(
        "1.19.2",
        "/Users/x/.claude/plugins/cache/system-design/sd/1.19.2",
    )
}

pub(super) fn check_with(id: &StepId, rig: &Rig, out: &str) -> CheckResult {
    let step = find(&steps(&rig.machine, Some(rig.all.clone())), id);
    let (_, parse) = remote(&step);
    parse(
        &RemoteOutput {
            text: out.into(),
            code: 0,
        },
        &rig.step(),
    )
}

pub(super) fn check(id: &StepId, out: &str) -> CheckResult {
    check_with(
        id,
        &Rig::new(linux("vps-sing-1")).with_mac(default_mac()),
        out,
    )
}

pub(super) fn script(id: &StepId, m: &Machine) -> String {
    remote(&find(&steps(m, None), id)).0
}

pub(super) fn fix(id: &StepId, m: &Machine) -> super::step::FixPlan {
    plan(&find(&steps(m, None), id))
}

pub(super) fn plugins_json(sd: Option<&str>, sp: Option<&str>) -> String {
    let mut entries: Vec<String> = Vec::new();
    if let Some(sp) = sp {
        entries.push(format!(
            r#""superpowers@claude-plugins-official":[{{"scope":"user","installPath":"/x","version":{}}}]"#,
            json!(sp)
        ));
    }
    if let Some(sd) = sd {
        entries.push(format!(
            r#""sd@system-design":[{{"scope":"user","installPath":"/y","version":{}}}]"#,
            json!(sd)
        ));
    }
    format!(r#"{{"version":2,"plugins":{{{}}}}}"#, entries.join(","))
}

pub(super) fn plugins_out(sd: &str, sp: &str) -> String {
    format!("{}\n@@sd\n{SD_LINE}", plugins_json(Some(sd), Some(sp)))
}

pub(super) fn text(r: &CheckResult) -> &str {
    r.state.text()
}

// MARK: MacFacts and plugins

#[test]
fn mac_facts_reads_versions_and_paths() {
    let tmp = TempDir::new("facts");
    let home = tmp.path().join("home");
    let plugins = home.join(".claude/plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    std::fs::copy(
        fixture_file("installed_plugins.json"),
        plugins.join("installed_plugins.json"),
    )
    .unwrap();
    let market = tmp.path().join("system-design");
    std::fs::create_dir_all(market.join(".claude-plugin")).unwrap();
    std::fs::write(
        market.join(".claude-plugin/marketplace.json"),
        r#"{"name":"system-design"}"#,
    )
    .unwrap();
    let known = std::fs::read_to_string(fixture_file("known_marketplaces.json"))
        .unwrap()
        .replace("@SYSTEM_DESIGN_DIR@", &market.to_string_lossy());
    std::fs::write(plugins.join("known_marketplaces.json"), known).unwrap();

    let facts = mac_facts::load(&home);
    assert_eq!(
        facts.plugins["sd@system-design"],
        plugin(
            "1.19.2",
            "/Users/x/.claude/plugins/cache/system-design/sd/1.19.2"
        )
    );
    // the user-scope entry, not the project one listed first
    assert_eq!(
        facts.plugins["superpowers@claude-plugins-official"].version,
        "7.0.0"
    );
    assert_eq!(
        facts.marketplace_json["system-design"],
        r#"{"name":"system-design"}"#
    );
}

#[test]
fn mac_facts_with_nothing_installed_is_empty() {
    let tmp = TempDir::new("nowhere");
    let facts = mac_facts::load(&tmp.path().join("nowhere"));
    assert!(facts.plugins.is_empty());
    assert!(facts.marketplace_json.is_empty());
    // text that isn't the files' shape gives nothing, too
    assert!(mac_facts::installed_plugins("not json").is_empty());
    assert!(mac_facts::installed_plugins(
        r#"{"plugins":{"a":[{"scope":"project","version":"1","installPath":"/x"}]}}"#
    )
    .is_empty());
    assert_eq!(
        mac_facts::system_design_dir(r#"{"system-design":{"source":{"source":"github"}}}"#),
        None
    );
}

#[test]
fn plugins_pass_only_at_the_macs_versions() {
    let old = check(&StepId::PLUGINS, &plugins_out("1.19.1", "7.0.0"));
    assert_eq!(
        old.state,
        CheckState::Fail("sd 1.19.1 here, 1.19.2 on this Mac".into())
    );

    let good = check(&StepId::PLUGINS, &plugins_out("1.19.2", "7.0.0"));
    assert_eq!(
        good.state,
        CheckState::Pass("superpowers 7.0.0, sd 1.19.2".into())
    );
    assert_eq!(
        good.detected.sd_path.as_deref(),
        Some("/home/agent/.local/share/agents/system-design/plugin/scripts/sd.py")
    );

    let none = check(&StepId::PLUGINS, "\n@@sd\n");
    assert_eq!(
        none.state,
        CheckState::Fail(
            "superpowers missing, 7.0.0 on this Mac; sd missing, 1.19.2 on this Mac".into()
        )
    );
}

#[test]
fn a_newer_superpowers_passes_but_an_older_or_unreadable_one_fails_and_sd_stays_exact() {
    let c = |sd: &str, sp: &str| check(&StepId::PLUGINS, &plugins_out(sd, sp));
    assert_eq!(
        c("1.19.2", "7.0.1").state,
        CheckState::Pass("superpowers 7.0.1, sd 1.19.2".into())
    );
    assert_eq!(
        c("1.19.2", "7.10.0").state,
        CheckState::Pass("superpowers 7.10.0, sd 1.19.2".into())
    );
    assert_eq!(
        c("1.19.2", "6.9.0").state,
        CheckState::Fail("superpowers 6.9.0 here, 7.0.0 on this Mac".into())
    );
    let unreadable = c("1.19.2", "7.0.0;x");
    assert!(
        matches!(&unreadable.state, CheckState::Fail(w) if w.contains("superpowers") && w.contains("7.0.0 on this Mac"))
    );
    // sd is never "newer is fine": a newer sd here still fails.
    assert_eq!(
        c("1.19.3", "7.0.0").state,
        CheckState::Fail("sd 1.19.3 here, 1.19.2 on this Mac".into())
    );
}

/// An sd older than 1.19.3 on the machine would run `--check` of that version, which creates
/// labels. Plugins installed reports the machine's sd version; the setup check holds back on it.
#[test]
fn plugins_reports_the_machines_sd_version() {
    let ok = check(&StepId::PLUGINS, &plugins_out("1.19.2", "7.0.0"));
    assert_eq!(ok.detected.sd_version.as_deref(), Some("1.19.2"));
    let old = check(&StepId::PLUGINS, &plugins_out("1.19.1", "7.0.0"));
    assert_eq!(
        old.detected.sd_version.as_deref(),
        Some("1.19.1"),
        "reported whether or not it passes"
    );
    let none = check(&StepId::PLUGINS, &format!("\n@@sd\n{SD_LINE}"));
    assert_eq!(none.detected.sd_version, None);
    let odd = check(&StepId::PLUGINS, &plugins_out("1.19;touch x", "7.0.0"));
    assert_eq!(
        odd.detected.sd_version, None,
        "an unreadable version is not reported"
    );
}

#[test]
fn the_setup_check_holds_back_while_the_machines_sd_is_older_than_the_minimum() {
    let m = linux("vps-sing-1");
    let step = find(&steps(&m, None), &StepId::SD_CHECK);
    let hold = step.hold_when.expect("a hold");
    let at = |v: Option<&str>| {
        hold(&Detected {
            sd_version: v.map(str::to_string),
            ..Detected::default()
        })
    };
    let why = format!("sd on vps-sing-1 is older than {MIN_SD}; update plugins first");
    assert_eq!(at(Some("1.19.2")), Some(why.clone()));
    assert_eq!(at(Some("1.18.9")), Some(why));
    assert_eq!(at(Some("1.19.3")), None);
    assert_eq!(at(Some("1.19.10")), None, "number by number, not text");
    assert_eq!(at(Some("2.0.0")), None);
    assert_eq!(
        at(None),
        None,
        "no version known: the steps before it say why"
    );
}

#[test]
fn plugins_cant_tell_when_this_mac_lacks_one() {
    let mut bare = default_mac();
    bare.plugins.remove("superpowers@claude-plugins-official");
    let rig = Rig::new(linux("vps-sing-1")).with_mac(bare);
    let r = check_with(&StepId::PLUGINS, &rig, &plugins_out("1.19.2", "7.0.0"));
    assert!(matches!(r.state, CheckState::CantTell(_)), "{:?}", r.state);
}

#[test]
fn plugins_fail_when_sd_py_is_not_where_the_step_puts_it() {
    let r = check(
        &StepId::PLUGINS,
        &format!("{}\n@@sd\n", plugins_json(Some("1.19.2"), Some("7.0.0"))),
    );
    assert!(r.state.is_fail());
    assert_eq!(r.detected.sd_path, None);
}

#[test]
fn an_unsafe_sd_path_is_never_detected() {
    let out = format!(
        "{}\n@@sd\nsd=/home/a;touch /tmp/x\n",
        plugins_json(Some("1.19.2"), Some("7.0.0"))
    );
    assert_eq!(check(&StepId::PLUGINS, &out).detected.sd_path, None);
}

#[test]
fn the_plugins_probe_is_the_briefs_script() {
    let s = script(&StepId::PLUGINS, &linux("vps-sing-1"));
    assert!(
        s.contains(r#"head -c 1048576 "$HOME/.claude/plugins/installed_plugins.json" 2>/dev/null"#)
    );
    assert!(s.contains("echo @@sd"));
    assert!(s.contains(r#"[ -f "$d" ] && printf 'sd=%s\n' "$d""#));
}

#[test]
fn plugin_versions_from_the_machine_never_reach_a_script() {
    let hostile = "1.0';touch /tmp/x;'";
    let r = check(&StepId::PLUGINS, &plugins_out(hostile, "7.0.0"));
    assert_eq!(
        r.state,
        CheckState::Fail("sd (unreadable version) here, 1.19.2 on this Mac".into())
    );
    assert!(!format!("{r:?}").contains("touch /tmp/x"));

    // the Fix takes only this Mac's facts: its display and scripts hold nothing from the machine
    let rig = Rig::new(linux("vps-sing-1")).with_mac(default_mac());
    let p = fix(&StepId::PLUGINS, &rig.machine);
    assert!(!p.display.contains("touch /tmp/x"));
    let _ = (p.run)(&rig.fix());
    for call in rig.argvs() {
        assert!(!call.join(" ").contains("touch /tmp/x"));
    }
}

/// A runner that answers `/usr/bin/tar` by writing the archive it was asked for.
pub(super) fn tar_rig(rig: &Rig) {
    rig.runner.on_call(
        |a| a[0] == "/usr/bin/tar",
        |call| {
            let i = call.argv.iter().position(|a| a == "-czf").unwrap();
            std::fs::write(&call.argv[i + 1], b"FAKE-TGZ").unwrap();
            CommandResult::ok("")
        },
    );
}

#[test]
fn plugins_fix_ships_this_macs_sd() {
    let tmp = TempDir::new("ship");
    let sd = sd_folder(&tmp, "sd-1.19.2");
    let mut facts = mac_facts("1.19.2", &sd);
    facts
        .marketplace_json
        .insert("system-design".into(), r#"{"name":"it's"}"#.into());
    let rig = Rig::new(linux("vps-sing-1")).with_mac(facts);
    tar_rig(&rig);
    let outcome = (fix(&StepId::PLUGINS, &rig.machine).run)(&rig.fix());
    assert!(outcome.ok, "{}", outcome.message);

    let calls = rig.calls();
    let tar: &Recorded = calls
        .iter()
        .find(|c| c.argv[0] == "/usr/bin/tar")
        .expect("tar ran");
    assert!(tar.argv.contains(&sd));
    assert!(
        tar.env.contains(&("COPYFILE_DISABLE".into(), "1".into())),
        "no ._ metadata entries in the archive"
    );
    let ssh = calls
        .iter()
        .find(|c| c.argv[0] == "/usr/bin/ssh")
        .expect("ssh ran");
    assert_eq!(
        ssh.stdin.as_deref(),
        Some(&b"FAKE-TGZ"[..]),
        "the archive goes on stdin"
    );
    let text = last(&ssh.argv);
    assert!(text.contains("claude plugin install sd@system-design"));
    assert!(text.contains("claude plugin install superpowers@claude-plugins-official"));
    // the old folder is set aside, not deleted, before the new one goes in; a failed move puts it back
    assert!(!text.contains(r#"rm -rf "$d"; "#));
    assert!(text.contains(r#"mv "$d" "$d.old""#));
    assert!(text.contains(r#"mv "$d.old" "$d""#));
    // the marketplace text is quoted, not spliced: a bare apostrophe never shows
    assert!(!text.contains("it's"));
    // the temp archive is gone
    let archive = &tar.argv[tar.argv.iter().position(|a| a == "-czf").unwrap() + 1];
    assert!(!std::path::Path::new(archive).exists());
}

#[test]
fn plugins_fix_without_this_macs_sd_fails_before_any_command() {
    let rig = Rig::new(linux("vps-sing-1"));
    let outcome = (fix(&StepId::PLUGINS, &rig.machine).run)(&rig.fix());
    assert!(!outcome.ok);
    assert!(rig.calls().is_empty());
}

#[test]
fn plugins_fix_with_a_failed_tar_sends_nothing_to_the_machine() {
    let tmp = TempDir::new("badtar");
    let rig = Rig::new(linux("vps-sing-1")).with_mac(mac_facts("1.19.2", &sd_folder(&tmp, "sd")));
    rig.runner.on(
        |a| a[0] == "/usr/bin/tar",
        CommandResult::new(1, "", "tar: boom"),
    );
    let outcome = (fix(&StepId::PLUGINS, &rig.machine).run)(&rig.fix());
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("couldn't pack sd 1.19.2"),
        "{}",
        outcome.message
    );
    assert!(rig.calls().iter().all(|c| c.argv[0] != "/usr/bin/ssh"));
}
