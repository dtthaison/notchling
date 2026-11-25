use super::fakes_b::*;
use super::registry::{AdminAccess, Machine};
use super::runner::CommandResult;
use super::service_files::{self, ServiceFile};
use super::shell;
use super::step::{CheckResult, FixOutcome, RemoteOutput, StepId};
use super::steps_sd::{self, MIN_SD};
use super::steps_sd_service::{install_script, MAC_DISABLE_SCRIPT};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) fn templates() -> BTreeMap<String, String> {
    service_files::templates(&templates_fixture()).expect("fixture templates")
}

pub(super) fn render_linux(user: &str, home: &str) -> Result<Vec<ServiceFile>, String> {
    service_files::render(
        &templates(),
        true,
        user,
        home,
        "/usr/bin/python3",
        &shell::sd_py(home),
    )
}

pub(super) fn render_mac() -> Vec<ServiceFile> {
    service_files::render(
        &templates(),
        false,
        "tester",
        "/Users/tester",
        "/opt/homebrew/bin/python3",
        &shell::sd_py("/Users/tester"),
    )
    .unwrap()
}

pub(super) fn digest(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

pub(super) fn step_of(m: &Machine, id: &StepId) -> super::step::SetupStep {
    find(
        &steps_sd::all(m, m.sd_worker().unwrap(), std::slice::from_ref(m)),
        id,
    )
}

pub(super) fn service_check(rig: &Rig, out: &str) -> CheckResult {
    let (_, parse) = remote(&step_of(&rig.machine, &StepId::SERVICE));
    parse(
        &RemoteOutput {
            text: out.into(),
            code: 0,
        },
        &rig.step(),
    )
}

pub(super) fn run_service(rig: &Rig) -> FixOutcome {
    (plan(&step_of(&rig.machine, &StepId::SERVICE)).run)(&rig.fix())
}

pub(super) fn rig_with_templates(m: Machine, tmp: TempDir) -> Rig {
    let facts = mac_facts("1.19.2", &sd_folder(&tmp, "sd"));
    let mut rig = Rig::new(m).with_mac(facts);
    rig.tmp = tmp;
    rig
}

#[test]
fn service_files_for_linux() {
    let files = render_linux("agent", "/home/agent").unwrap();
    assert_eq!(files.len(), 4);
    assert!(files
        .iter()
        .all(|f| f.remote_path == format!("/etc/systemd/system/{}", f.name)));
    let mut names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "sd-testflight.service",
            "sd-testflight.timer",
            "sd-worker.service",
            "sd-worker.slice"
        ]
    );
    let service = files
        .iter()
        .find(|f| f.name == "sd-worker.service")
        .unwrap();
    let lines: Vec<&str> = service.content.split('\n').collect();
    assert!(lines.contains(&"User=agent"));
    assert!(lines.contains(&"ExecStart=/usr/bin/python3 /home/agent/.local/share/agents/system-design/plugin/scripts/sd.py worker run"));
    let path_line = format!("Environment=PATH={}", shell::service_path("/home/agent"));
    assert!(lines.contains(&path_line.as_str()));
    for f in &files {
        assert!(
            !(f.content.contains("@SD_PY@")
                || f.content.contains("@PYTHON3@")
                || f.content.contains("@HOME@")),
            "{}",
            f.name
        );
        assert!(!f.content.contains("EnvironmentFile"), "{}", f.name);
    }
}

#[test]
fn service_files_set_the_user() {
    let files = render_linux("worker", "/home/worker").unwrap();
    for name in ["sd-worker.service", "sd-testflight.service"] {
        let f = files.iter().find(|f| f.name == name).unwrap();
        let lines: Vec<&str> = f.content.split('\n').collect();
        assert!(lines.contains(&"User=worker"), "{name}");
        assert!(!lines.contains(&"User=agent"), "{name}");
    }
}

#[test]
fn service_files_refuse_root_and_old_templates() {
    let sd = shell::sd_py("/home/agent");
    let render = |t: &BTreeMap<String, String>, user: &str, home: &str| {
        service_files::render(t, true, user, home, "/usr/bin/python3", &sd)
    };
    assert_eq!(
        render(&templates(), "root", "/root").unwrap_err(),
        "the worker must not run as root"
    );
    let mut old = templates();
    old.get_mut("sd-worker.service")
        .unwrap()
        .push_str("EnvironmentFile=/home/agent/.config/agents/claude.env\n");
    assert_eq!(
        render(&old, "agent", "/home/agent").unwrap_err(),
        format!("sd on this Mac is older than {MIN_SD}; update it first")
    );
    let mut leftover = templates();
    leftover
        .get_mut("sd-worker.slice")
        .unwrap()
        .push_str("Foo=@MYSTERY_1@\n");
    let why = render(&leftover, "agent", "/home/agent").unwrap_err();
    assert!(why.contains("@MYSTERY_1@"), "{why}");
    let mut no_user = templates();
    let svc = no_user.get_mut("sd-worker.service").unwrap();
    *svc = svc.replace("User=agent\n", "");
    assert!(
        render(&no_user, "agent", "/home/agent").is_err(),
        "a unit with no User= passed"
    );
}

#[test]
fn service_files_refuse_unsafe_values() {
    let t = templates();
    let r = |linux: bool, user: &str, home: &str, python: &str, sd: &str| {
        service_files::render(&t, linux, user, home, python, sd)
    };
    assert!(r(
        true,
        "agent\nExecStartPre=/bin/x",
        "/home/agent",
        "/usr/bin/python3",
        "/a/sd.py"
    )
    .is_err());
    assert!(r(
        true,
        "agent",
        "/home/agent",
        "/usr/bin/python3 --evil",
        "/a/sd.py"
    )
    .is_err());
    assert!(r(true, "agent", "/home/a b", "/usr/bin/python3", "/a/sd.py").is_err());
    assert!(r(true, "agent", "/home/a b;x", "/usr/bin/python3", "/a/sd.py").is_err());
    assert!(r(true, "agent", "/home/agent", "python3", "/a/sd.py").is_err());
    assert!(r(
        false,
        "agent",
        "/home/agent",
        "/usr/bin/python3",
        "/a/sd.py; touch x"
    )
    .is_err());
}

#[test]
fn service_files_for_mac() {
    let sd = shell::sd_py("/Users/tester");
    let files = render_mac();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "com.sd.worker.plist");
    assert_eq!(
        files[0].remote_path,
        "~/Library/LaunchAgents/com.sd.worker.plist"
    );
    let c = &files[0].content;
    assert!(c.contains(&format!(
        "<string>/opt/homebrew/bin/python3</string>\n\t\t<string>{sd}</string>\n\t\t<string>worker</string>\n\t\t<string>run</string>"
    )), "{c}");
    assert!(
        c.contains(&format!(
            "<key>PATH</key>\n\t\t<string>{}</string>",
            shell::service_path("/Users/tester")
        )),
        "{c}"
    );
    assert_eq!(
        c.matches("<string>/Users/tester/Library/Logs/sd-worker.log</string>")
            .count(),
        2
    );
    assert!(!c.contains('@'));
    assert!(!c.contains("EnvironmentFile"));
    assert!(
        !c.contains("<!--"),
        "the comments with the placeholders are dropped"
    );
}

#[test]
fn a_plist_that_is_cut_or_has_no_path_is_refused() {
    let r = |t: &BTreeMap<String, String>| {
        service_files::render(
            t,
            false,
            "tester",
            "/Users/tester",
            "/opt/homebrew/bin/python3",
            &shell::sd_py("/Users/tester"),
        )
    };
    let mut cut = templates();
    let p = cut.get_mut("com.sd.worker.plist").unwrap();
    *p = p.replace("</dict>\n</plist>", "");
    assert!(r(&cut).is_err());
    let mut no_path = templates();
    let p = no_path.get_mut("com.sd.worker.plist").unwrap();
    *p = p.replace("<key>PATH</key>", "<key>OTHER</key>");
    assert!(r(&no_path).is_err());
}

#[test]
fn templates_are_read_from_a_directory_and_none_when_one_is_missing() {
    assert_eq!(templates().len(), 5);
    let tmp = TempDir::new("notemplates");
    assert!(service_files::templates(tmp.path()).is_none());
}

#[test]
fn linux_service_fix_only_writes_the_four_units_and_reloads() {
    let files = render_linux("agent", "/home/agent").unwrap();
    let script = install_script(&files);
    let lines: Vec<&str> = script.split('\n').collect();
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.contains("base64 -d > /etc/systemd/system/"))
            .count(),
        4
    );
    assert!(script.contains("systemctl daemon-reload"));
    assert!(script.starts_with("set -e\numask 022\n"));
    let outside: Vec<String> = lines
        .iter()
        .map(|l| match l.strip_prefix("printf '%s' '") {
            Some(rest) => match rest.find('\'') {
                Some(i) => format!("printf '%s' ''{}", &rest[i + 1..]),
                None => l.to_string(),
            },
            None => l.to_string(),
        })
        .collect();
    assert!(
        outside.contains(
            &"printf '%s' '' | base64 -d > /etc/systemd/system/sd-worker.service.new".to_string()
        ),
        "{}",
        outside.join("\n")
    );
    let joined = outside.join("\n");
    for word in ["enable", "start", "restart", "stop", "disable"] {
        assert!(!joined.contains(word), "{word}");
    }
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.starts_with("mv -f /etc/systemd/system/"))
            .count(),
        4
    );
    for service in shell::PRODUCTION_SERVICES {
        assert!(!script.contains(service), "{service}");
    }
    assert_eq!(script.matches("systemctl").count(), 1);
}

#[test]
fn linux_service_fix_runs_one_admin_call_as_root_with_no_stdin() {
    let tmp = TempDir::new("linuxfix");
    let mut vps = linux("vps-sing-1");
    vps.set_admin(AdminAccess::RootSsh);
    let rig = rig_with_templates(vps, tmp);
    let outcome = run_service(&rig);
    assert!(outcome.ok, "{}", outcome.message);
    let calls = rig.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].argv.contains(&"root@vps-sing-1.test".to_string()));
    assert!(!last(&calls[0].argv).contains("sudo"));
    assert!(calls[0].stdin.is_none());
}

#[test]
fn linux_service_fix_through_sudo_refuses_a_worker_that_is_the_admin() {
    let tmp = TempDir::new("linuxsudo");
    let rig = rig_with_templates(linux("vps-sing-1"), tmp);
    let outcome = run_service(&rig);
    assert!(!outcome.ok);
    assert!(rig.calls().is_empty(), "nothing ran");
}

#[test]
fn service_fix_on_a_mac_writes_only_the_launch_agent_without_admin() {
    let tmp = TempDir::new("macfix");
    let rig = rig_with_templates(mac("m1"), tmp);
    let p = plan(&step_of(&rig.machine, &StepId::SERVICE));
    assert!(!p.needs_admin);
    let outcome = (p.run)(&rig.fix());
    assert!(outcome.ok, "{}", outcome.message);
    let calls = rig.calls();
    assert!(last(&calls[0].argv).contains("print-disabled"));
    assert!(last(&calls[1].argv).contains(MAC_DISABLE_SCRIPT));
    assert!(calls[1].stdin.is_none());
    let write = calls.last().unwrap();
    assert!(last(&write.argv).contains("com.sd.worker.plist"));
    assert!(last(&write.argv).contains("chmod 644"));
    assert!(!last(&write.argv).contains("launchctl"));
    let plist = write.stdin_text();
    assert!(plist.contains("<string>/opt/homebrew/bin/python3</string>"));
    assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));
    assert!(plist.contains("<key>KeepAlive</key>\n\t<true/>"));
    assert_eq!(calls.len(), 3, "the state probe, the disable, the write");
    let all: String = rig
        .argvs()
        .iter()
        .map(|a| a.join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    for verb in ["bootstrap", "load", "enable", "kickstart", "bootout"] {
        assert!(!all.contains(&format!("launchctl {verb}")), "{verb}");
    }
    assert_eq!(
        MAC_DISABLE_SCRIPT,
        r#"launchctl disable "gui/$(id -u)/com.sd.worker""#
    );
    assert!(p.display.contains(MAC_DISABLE_SCRIPT));
}

#[test]
fn service_fix_on_a_mac_leaves_no_plist_when_the_disable_fails() {
    let tmp = TempDir::new("macdisable");
    let rig = rig_with_templates(mac("m1"), tmp);
    rig.runner.on(
        script_has("launchctl disable"),
        CommandResult::new(1, "", "no GUI domain\n"),
    );
    let outcome = run_service(&rig);
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("no GUI domain"),
        "{}",
        outcome.message
    );
    assert!(
        outcome.message.contains("no service file was written"),
        "{}",
        outcome.message
    );
    let calls = rig.calls();
    assert_eq!(
        calls.len(),
        2,
        "the state probe and the failed disable; stopped before the write"
    );
    assert!(calls.iter().all(|c| c.stdin.is_none()));
    assert!(!calls
        .iter()
        .any(|c| c.argv.join("").contains("LaunchAgents")));
}
