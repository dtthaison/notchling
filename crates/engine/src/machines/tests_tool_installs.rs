use super::engine_plan::{AdminAnswer, PlanKind};
use super::fakes::TempDir;
use super::runner::CommandResult;
use super::step::{FixOutcome, StepId};
use super::tests_engine_world::*;
use super::tool_installs::ToolInstall;
use crate::versions::{Tool, ToolState, Version};
use std::sync::{Arc, Mutex};

const PROBE: &str = "for t in claude codex agy notchling";

struct Rig {
    w: World,
    codex: Arc<Mutex<Option<String>>>,
    agy: Arc<Mutex<bool>>,
    commands: Arc<Mutex<Vec<String>>>,
    audit: TempDir,
}

struct Behavior {
    codex_after: Option<&'static str>,
    agy_after: bool,
    code: i32,
}

fn ver(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn want(tool: Tool, mac: Option<&str>) -> PlanKind {
    PlanKind::InstallTool(ToolInstall {
        tool,
        mac: mac.map(ver),
    })
}

fn probe_text(codex: Option<&str>, agy: bool) -> String {
    let mut text = String::from("claude.path=\n");
    match codex {
        Some(v) => text.push_str(&format!(
            "codex.path=/home/w/.local/bin/codex\ncodex.link=/home/w/.codex/packages/standalone/releases/{v}/codex\n"
        )),
        None => text.push_str("codex.path=\n"),
    }
    if agy {
        text.push_str("agy.path=/home/w/.local/bin/agy\nagy.link=\n");
    } else {
        text.push_str("agy.path=\n");
    }
    text.push_str("notchling.path=\n");
    if let Some(v) = codex {
        text.push_str(&format!("codex.version=codex-cli {v}\n"));
    }
    text
}

fn rig(b: Behavior) -> Rig {
    let audit = TempDir::new("tool-installs");
    let mut setup = Setup::new(|_, _| Vec::new());
    setup.machines = vec![linux("m1")];
    setup.audit = Some(audit.join("audit.jsonl"));
    let codex: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let agy = Arc::new(Mutex::new(false));
    let commands = Arc::new(Mutex::new(Vec::new()));
    let (c, a) = (codex.clone(), agy.clone());
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains(PROBE)),
        move |_| CommandResult::ok(&probe_text(lock(&c).as_deref(), *lock(&a))),
    );
    let (c, a, log) = (codex.clone(), agy.clone(), commands.clone());
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("curl -fsSL")),
        move |call| {
            lock(&log).push(
                call.argv
                    .last()
                    .map(|s| s.replace("'\\''", "'"))
                    .unwrap_or_default(),
            );
            if b.code == 0 {
                *lock(&c) = b.codex_after.map(str::to_string);
                *lock(&a) = b.agy_after;
                CommandResult::ok("installing\n")
            } else {
                CommandResult::new(b.code, "installing\n", "EACCES: permission denied\n")
            }
        },
    );
    Rig {
        w: setup.build(),
        codex,
        agy,
        commands,
        audit,
    }
}

impl Rig {
    fn read(&self) {
        self.w.engine.read_worker_tools("m1").unwrap();
    }

    fn install(&self, kind: PlanKind) -> FixOutcome {
        self.read();
        self.w.click("m1", kind, AdminAnswer::None)
    }

    fn commands(&self) -> Vec<String> {
        lock(&self.commands).clone()
    }

    fn audit_lines(&self) -> Vec<String> {
        std::fs::read_to_string(self.audit.join("audit.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

fn codex_ok(after: &'static str) -> Behavior {
    Behavior {
        codex_after: Some(after),
        agy_after: false,
        code: 0,
    }
}

#[test]
fn codex_installs_the_macs_version_and_succeeds_only_when_the_reread_equals_it() {
    let r = rig(codex_ok("0.166.0"));
    let out = r.install(want(Tool::Codex, Some("0.166.0")));
    assert!(out.ok, "{}", out.message);
    assert_eq!(out.message, "Codex 0.166.0 is installed on m1");
    let commands = r.commands();
    assert_eq!(commands.len(), 1);
    assert!(
        commands[0].contains("sh -s -- --release '0.166.0'"),
        "{}",
        commands[0]
    );
    let ssh =
        r.w.runner
            .calls()
            .into_iter()
            .find(|c| c.argv.last().is_some_and(|s| s.contains("curl -fsSL")))
            .unwrap();
    assert_eq!(ssh.argv[0], "/usr/bin/ssh");
    assert!(!ssh.text().contains("sudo"));
    match &r.w.engine.worker_tools("m1").unwrap().tools[&Tool::Codex] {
        ToolState::Installed { version, .. } => assert_eq!(version.as_ref(), Some(&ver("0.166.0"))),
        other => panic!("{other:?}"),
    }
    assert_eq!(r.w.engine.running("m1"), None);
}

#[test]
fn another_version_after_a_pinned_install_is_a_failure() {
    let r = rig(codex_ok("0.165.0"));
    let out = r.install(want(Tool::Codex, Some("0.166.0")));
    assert!(!out.ok);
    assert!(
        out.message.contains("reports 0.165.0, not 0.166.0"),
        "{}",
        out.message
    );
}

#[test]
fn an_install_that_leaves_the_tool_missing_is_a_failure() {
    let r = rig(Behavior {
        codex_after: None,
        agy_after: false,
        code: 0,
    });
    let out = r.install(want(Tool::Codex, Some("0.166.0")));
    assert!(!out.ok);
    assert!(
        out.message.contains("still isn't found on m1"),
        "{}",
        out.message
    );
}

#[test]
fn a_failed_installer_shows_its_error_and_the_npm_hint() {
    let r = rig(Behavior {
        codex_after: None,
        agy_after: false,
        code: 1,
    });
    let out = r.install(want(Tool::Codex, Some("0.166.0")));
    assert!(!out.ok);
    assert!(
        out.message.contains("didn't install on m1"),
        "{}",
        out.message
    );
    assert!(out.message.contains("EACCES"), "{}", out.message);
    assert!(out.message.contains("npm can't write"), "{}", out.message);
    assert_eq!(*lock(&r.codex), None);
}

#[test]
fn antigravity_installs_the_newest_and_succeeds_when_found() {
    let r = rig(Behavior {
        codex_after: None,
        agy_after: true,
        code: 0,
    });
    r.read();
    let plan =
        r.w.engine
            .plan("m1", want(Tool::Agy, Some("1.5.9")))
            .unwrap();
    assert_eq!(plan.steps[0].title, "Install Antigravity on m1");
    assert_eq!(plan.steps[0].id, StepId::tool_install(Tool::Agy));
    assert!(plan.steps[0]
        .display
        .contains("antigravity.google/cli/install.sh"));
    assert!(!plan.steps[0].display.contains("1.5.9"));
    let out =
        r.w.engine
            .run(r.w.engine.confirm(plan.id, AdminAnswer::None).unwrap());
    assert!(out.ok, "{}", out.message);
    assert_eq!(out.message, "Antigravity is installed on m1");
    assert!(*lock(&r.agy));
}

#[test]
fn the_plan_shows_the_exact_command_and_runs_nothing() {
    let r = rig(codex_ok("0.166.0"));
    r.read();
    let plan =
        r.w.engine
            .plan("m1", want(Tool::Codex, Some("0.166.0")))
            .unwrap();
    assert_eq!(plan.title, "Install Codex on m1");
    assert_eq!(plan.steps.len(), 1);
    assert!(plan.steps[0].display.contains("--release '0.166.0'"));
    assert!(!plan.steps[0].needs_admin);
    assert!(r.commands().is_empty());
}

#[test]
fn a_plan_needs_the_tools_read_and_a_tool_that_is_missing() {
    let r = rig(codex_ok("0.166.0"));
    let why = r.w.engine.plan("m1", want(Tool::Codex, None)).unwrap_err();
    assert!(why.contains("aren't read yet"), "{why}");
    assert!(r.w.engine.plan("nobody", want(Tool::Codex, None)).is_err());
    r.read();
    *lock(&r.codex) = Some("0.118.0".into());
    r.read();
    let why = r.w.engine.plan("m1", want(Tool::Codex, None)).unwrap_err();
    assert!(why.contains("already installed"), "{why}");
    assert!(r.w.engine.plan("m1", want(Tool::Claude, None)).is_err());
}

#[test]
fn a_version_that_is_not_plain_never_reaches_a_command() {
    let r = rig(codex_ok("0.166.0"));
    r.read();
    for bad in ["0.166.0-alpha.1", "0.166.0-rc"] {
        let why =
            r.w.engine
                .plan("m1", want(Tool::Codex, Some(bad)))
                .unwrap_err();
        assert!(why.contains("isn't a version"), "{why}");
    }
    assert!(r.commands().is_empty());
}

#[test]
fn a_machine_that_changed_since_the_plan_needs_a_look() {
    let r = rig(codex_ok("0.166.0"));
    r.read();
    let plan =
        r.w.engine
            .plan("m1", want(Tool::Codex, Some("0.166.0")))
            .unwrap();
    *lock(&r.codex) = Some("0.118.0".into());
    r.read();
    let out =
        r.w.engine
            .run(r.w.engine.confirm(plan.id, AdminAnswer::None).unwrap());
    assert!(!out.ok);
    assert!(out.message.contains("needs a look"), "{}", out.message);
    assert!(r.commands().is_empty());
}

#[test]
fn the_run_goes_into_the_log_and_the_audit() {
    let r = rig(codex_ok("0.166.0"));
    let out = r.install(want(Tool::Codex, Some("0.166.0")));
    assert!(out.ok);
    let log = r.w.log("m1");
    assert!(log.contains("▸ Install Codex on m1"), "{log}");
    assert!(log.contains("✓ Codex 0.166.0 is installed on m1"), "{log}");
    let audit = r.audit_lines();
    assert_eq!(audit.len(), 1, "{audit:?}");
    assert!(audit[0].contains("\"op\":\"machines.fix\""), "{}", audit[0]);
    assert!(audit[0].contains("Install Codex on m1"), "{}", audit[0]);
    assert!(audit[0].contains("\"ok\":true"), "{}", audit[0]);
}
