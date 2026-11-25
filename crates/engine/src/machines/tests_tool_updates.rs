use super::engine_plan::{AdminAnswer, PlanKind};
use super::fakes::TempDir;
use super::runner::CommandResult;
use super::step::StepId;
use super::tests_engine_world::*;
use super::tool_updates::ToolMove;
use crate::versions::{Method, Tool, Version};
use std::sync::{Arc, Mutex};

const PROBE: &str = "for t in claude codex agy notchling";

#[derive(Clone, Copy)]
enum Flavor {
    ClaudeNative,
    ClaudeNpm,
    ClaudeBrew,
    ClaudeUnknown,
    CodexStandalone,
    CodexNpm,
    CodexUnknown,
}

fn probe_text(flavor: Flavor, version: &str) -> String {
    let (claude, codex) = match flavor {
        Flavor::ClaudeNative => (
            Some((
                "/home/w/.local/bin/claude",
                format!("/home/w/.local/share/claude/versions/{version}"),
            )),
            None,
        ),
        Flavor::ClaudeNpm => (
            Some((
                "/usr/local/bin/claude",
                "../lib/node_modules/@anthropic-ai/claude-code/cli.js".to_string(),
            )),
            None,
        ),
        Flavor::ClaudeBrew => (
            Some((
                "/opt/homebrew/bin/claude",
                format!("/opt/homebrew/Caskroom/claude-code/{version}/claude"),
            )),
            None,
        ),
        Flavor::ClaudeUnknown => (Some(("/usr/bin/claude", String::new())), None),
        Flavor::CodexStandalone => (
            None,
            Some((
                "/home/w/.local/bin/codex",
                format!("/home/w/.codex/packages/standalone/releases/{version}/codex"),
            )),
        ),
        Flavor::CodexNpm => (
            None,
            Some((
                "/usr/local/bin/codex",
                "../lib/node_modules/@openai/codex/bin/codex.js".to_string(),
            )),
        ),
        Flavor::CodexUnknown => (None, Some(("/usr/bin/codex", String::new()))),
    };
    let mut text = String::new();
    match &claude {
        Some((path, link)) if !path.is_empty() => {
            text.push_str(&format!("claude.path={path}\nclaude.link={link}\n"))
        }
        _ => text.push_str("claude.path=\n"),
    }
    match &codex {
        Some((path, link)) => text.push_str(&format!("codex.path={path}\ncodex.link={link}\n")),
        None => text.push_str("codex.path=\n"),
    }
    text.push_str("agy.path=\nnotchling.path=\n");
    if claude.is_some() {
        text.push_str(&format!("claude.version={version} (Claude Code)\n"));
    }
    if codex.is_some() {
        text.push_str(&format!("codex.version=codex-cli {version}\n"));
    }
    text
}

struct Rig {
    w: World,
    version: Arc<Mutex<String>>,
    busy: Arc<Mutex<Option<usize>>>,
    commands: Arc<Mutex<Vec<String>>>,
    audit: TempDir,
}

fn rig(flavor: Flavor, start: &str, installs: Option<&'static str>, code: i32) -> Rig {
    let audit = TempDir::new("tool-updates");
    let mut setup = Setup::new(|_, _| Vec::new());
    setup.machines = vec![linux("m1")];
    setup.audit = Some(audit.join("audit.jsonl"));
    let version = Arc::new(Mutex::new(start.to_string()));
    let busy = Arc::new(Mutex::new(Some(0usize)));
    let commands = Arc::new(Mutex::new(Vec::new()));
    let (v, c) = (version.clone(), commands.clone());
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains(PROBE)),
        move |_| CommandResult::ok(&probe_text(flavor, &lock(&v).clone())),
    );
    let b = busy.clone();
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("busy=")),
        move |_| match *lock(&b) {
            Some(n) => CommandResult::ok(&format!("busy={n}\n")),
            None => CommandResult::new(1, "", "ps: not found"),
        },
    );
    let v = version.clone();
    setup.runner.on_call(
        |argv| {
            argv.last().is_some_and(|s| {
                s.contains("install") && !s.contains(PROBE) && !s.contains("busy=")
            })
        },
        move |call| {
            lock(&c).push(
                call.argv
                    .last()
                    .map(|s| s.replace("'\\''", "'"))
                    .unwrap_or_default(),
            );
            if code == 0 {
                if let Some(to) = installs {
                    *lock(&v) = to.to_string();
                }
            }
            CommandResult::new(code, "installing\n", if code == 0 { "" } else { "boom\n" })
        },
    );
    Rig {
        w: setup.build(),
        version,
        busy,
        commands,
        audit,
    }
}

fn ver(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn mv(tool: Tool, to: &str) -> ToolMove {
    ToolMove { tool, to: ver(to) }
}

impl Rig {
    fn read(&self) {
        self.w.engine.read_worker_tools("m1").unwrap();
    }

    fn update(&self, moves: Vec<ToolMove>) -> super::step::FixOutcome {
        self.read();
        self.w
            .click("m1", PlanKind::UpdateTools(moves), AdminAnswer::None)
    }

    fn installs(&self) -> Vec<String> {
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

#[test]
fn native_claude_runs_claude_install_and_succeeds_on_the_matching_reread() {
    let r = rig(Flavor::ClaudeNative, "2.1.318", Some("2.1.327"), 0);
    let out = r.update(vec![mv(Tool::Claude, "2.1.327")]);
    assert!(out.ok, "{}", out.message);
    assert_eq!(out.message, "Claude Code on m1 is now 2.1.327");
    let installs = r.installs();
    assert_eq!(installs.len(), 1, "{installs:?}");
    assert!(
        installs[0].contains("'claude' 'install' '2.1.327'"),
        "{}",
        installs[0]
    );
    let ssh =
        r.w.runner
            .calls()
            .into_iter()
            .find(|c| c.argv.last().is_some_and(|s| s.contains("install")))
            .unwrap();
    assert_eq!(ssh.argv[0], "/usr/bin/ssh");
    assert!(ssh.argv.iter().any(|a| a == "--"));
    assert!(!ssh.text().contains("sudo"));
    let known = r.w.engine.worker_tools("m1").unwrap();
    match &known.tools[&Tool::Claude] {
        crate::versions::ToolState::Installed {
            version, method, ..
        } => {
            assert_eq!(version.as_ref(), Some(&ver("2.1.327")));
            assert_eq!(*method, Method::Native);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(r.w.engine.running("m1"), None);
}

#[test]
fn npm_claude_and_npm_codex_pin_the_package() {
    let r = rig(Flavor::ClaudeNpm, "2.1.318", Some("2.1.327"), 0);
    assert!(r.update(vec![mv(Tool::Claude, "2.1.327")]).ok);
    assert!(
        r.installs()[0].contains("'npm' 'install' '-g' '@anthropic-ai/claude-code@2.1.327'"),
        "{:?}",
        r.installs()
    );
    let r = rig(Flavor::CodexNpm, "0.118.0", Some("0.166.0"), 0);
    assert!(r.update(vec![mv(Tool::Codex, "0.166.0")]).ok);
    assert!(
        r.installs()[0].contains("'npm' 'install' '-g' '@openai/codex@0.166.0'"),
        "{:?}",
        r.installs()
    );
}

#[test]
fn standalone_codex_runs_the_installer_with_the_release() {
    let r = rig(Flavor::CodexStandalone, "0.118.0", Some("0.166.0"), 0);
    let out = r.update(vec![mv(Tool::Codex, "0.166.0")]);
    assert!(out.ok, "{}", out.message);
    let command = &r.installs()[0];
    assert!(command.contains("install.sh"), "{command}");
    assert!(command.contains("--release 0.166.0"), "{command}");
}

#[test]
fn a_method_that_cannot_pin_gets_a_how_to_and_runs_nothing() {
    let cases = [
        (Flavor::ClaudeBrew, Tool::Claude, "Homebrew"),
        (Flavor::ClaudeUnknown, Tool::Claude, "can't tell how"),
        (Flavor::CodexUnknown, Tool::Codex, "can't tell how"),
    ];
    for (flavor, tool, word) in cases {
        let r = rig(flavor, "1.0.0", Some("2.0.0"), 0);
        r.read();
        let why =
            r.w.engine
                .plan("m1", PlanKind::UpdateTools(vec![mv(tool, "2.0.0")]))
                .unwrap_err();
        assert!(why.contains(word), "{why}");
        assert!(why.contains("m1"), "{why}");
        assert!(r.installs().is_empty());
    }
}

#[test]
fn a_running_tool_refuses_and_nothing_is_installed() {
    let r = rig(Flavor::ClaudeNative, "2.1.318", Some("2.1.327"), 0);
    *lock(&r.busy) = Some(2);
    let out = r.update(vec![mv(Tool::Claude, "2.1.327")]);
    assert!(!out.ok);
    assert!(out.message.contains("running on m1"), "{}", out.message);
    assert!(out.message.contains("2 processes"), "{}", out.message);
    assert!(r.installs().is_empty());
    assert_eq!(*lock(&r.version), "2.1.318");
}

#[test]
fn a_busy_probe_that_cannot_tell_refuses_too() {
    let r = rig(Flavor::ClaudeNative, "2.1.318", Some("2.1.327"), 0);
    *lock(&r.busy) = None;
    let out = r.update(vec![mv(Tool::Claude, "2.1.327")]);
    assert!(!out.ok);
    assert!(out.message.contains("Couldn't tell"), "{}", out.message);
    assert!(r.installs().is_empty());
}

#[test]
fn success_needs_the_reread_to_equal_the_target() {
    let r = rig(Flavor::ClaudeNative, "2.1.318", Some("2.1.320"), 0);
    let out = r.update(vec![mv(Tool::Claude, "2.1.327")]);
    assert!(!out.ok);
    assert!(
        out.message.contains("still reports 2.1.320, not 2.1.327"),
        "{}",
        out.message
    );
    let r = rig(Flavor::ClaudeNative, "2.1.318", None, 0);
    let out = r.update(vec![mv(Tool::Claude, "2.1.327")]);
    assert!(!out.ok);
    assert!(
        out.message.contains("still reports 2.1.318"),
        "{}",
        out.message
    );
}

#[test]
fn a_failed_install_shows_the_error_and_counts_as_failed() {
    let r = rig(Flavor::ClaudeNpm, "2.1.318", Some("2.1.327"), 1);
    let out = r.update(vec![mv(Tool::Claude, "2.1.327")]);
    assert!(!out.ok);
    assert!(
        out.message.contains("didn't install on m1"),
        "{}",
        out.message
    );
    assert!(out.message.contains("boom"), "{}", out.message);
    assert_eq!(*lock(&r.version), "2.1.318");
}

#[test]
fn a_version_that_is_not_strict_never_reaches_a_command() {
    let r = rig(Flavor::ClaudeNative, "2.1.318", Some("2.1.327"), 0);
    r.read();
    for bad in ["2.1.327-beta", "2.1.327-1"] {
        let why =
            r.w.engine
                .plan("m1", PlanKind::UpdateTools(vec![mv(Tool::Claude, bad)]))
                .unwrap_err();
        assert!(why.contains("isn't a version"), "{why}");
    }
    assert!(r.installs().is_empty());
}

#[test]
fn the_plan_shows_the_exact_command_and_runs_nothing() {
    let r = rig(Flavor::ClaudeNative, "2.1.318", Some("2.1.327"), 0);
    r.read();
    let plan =
        r.w.engine
            .plan(
                "m1",
                PlanKind::UpdateTools(vec![mv(Tool::Claude, "2.1.327")]),
            )
            .unwrap();
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].title, "Update Claude Code on m1 to 2.1.327");
    assert_eq!(plan.steps[0].display, "'claude' 'install' '2.1.327'");
    assert_eq!(plan.steps[0].id, StepId::tool_update(Tool::Claude));
    assert!(!plan.steps[0].needs_admin);
    assert!(r.installs().is_empty());
}

#[test]
fn a_plan_needs_the_tools_read_first_and_no_duplicates() {
    let r = rig(Flavor::ClaudeNative, "2.1.318", Some("2.1.327"), 0);
    let why =
        r.w.engine
            .plan(
                "m1",
                PlanKind::UpdateTools(vec![mv(Tool::Claude, "2.1.327")]),
            )
            .unwrap_err();
    assert!(why.contains("aren't read yet"), "{why}");
    r.read();
    for list in [
        vec![],
        vec![mv(Tool::Claude, "2.1.327"), mv(Tool::Claude, "2.1.328")],
        vec![mv(Tool::Codex, "0.166.0")],
        vec![mv(Tool::Notchling, "1.0.0")],
    ] {
        assert!(r.w.engine.plan("m1", PlanKind::UpdateTools(list)).is_err());
    }
    assert!(r
        .w
        .engine
        .plan(
            "nobody",
            PlanKind::UpdateTools(vec![mv(Tool::Claude, "2.1.327")])
        )
        .is_err());
}

#[test]
fn a_tool_already_at_the_target_is_not_planned() {
    let r = rig(Flavor::ClaudeNative, "2.1.327", Some("2.1.327"), 0);
    r.read();
    let why =
        r.w.engine
            .plan(
                "m1",
                PlanKind::UpdateTools(vec![mv(Tool::Claude, "2.1.327")]),
            )
            .unwrap_err();
    assert!(why.contains("already 2.1.327"), "{why}");
}

#[test]
fn the_run_goes_into_the_log_and_the_audit() {
    let r = rig(Flavor::ClaudeNative, "2.1.318", Some("2.1.327"), 0);
    let out = r.update(vec![mv(Tool::Claude, "2.1.327")]);
    assert!(out.ok, "{}", out.message);
    let log = r.w.log("m1");
    assert!(
        log.contains("▸ Update Claude Code on m1 to 2.1.327"),
        "{log}"
    );
    assert!(log.contains("$ 'claude' 'install' '2.1.327'"), "{log}");
    assert!(log.contains("✓ Claude Code on m1 is now 2.1.327"), "{log}");
    let audit = r.audit_lines();
    assert_eq!(audit.len(), 1, "{audit:?}");
    assert!(audit[0].contains("\"op\":\"machines.fix\""), "{}", audit[0]);
    assert!(
        audit[0].contains("Update Claude Code on m1 to 2.1.327"),
        "{}",
        audit[0]
    );
    assert!(audit[0].contains("\"ok\":true"), "{}", audit[0]);
    let r = rig(Flavor::ClaudeNative, "2.1.318", None, 0);
    let out = r.update(vec![mv(Tool::Claude, "2.1.327")]);
    assert!(!out.ok);
    assert!(r.audit_lines()[0].contains("\"ok\":false"));
}

#[test]
fn a_changed_machine_since_the_plan_needs_a_look() {
    let r = rig(Flavor::ClaudeNative, "2.1.318", Some("2.1.327"), 0);
    r.read();
    let plan =
        r.w.engine
            .plan(
                "m1",
                PlanKind::UpdateTools(vec![mv(Tool::Claude, "2.1.327")]),
            )
            .unwrap();
    *lock(&r.version) = "2.1.327".into();
    r.read();
    let out =
        r.w.engine
            .run(r.w.engine.confirm(plan.id, AdminAnswer::None).unwrap());
    assert!(!out.ok);
    assert!(out.message.contains("needs a look"), "{}", out.message);
    assert!(r.installs().is_empty());
}

fn both_rig(codex_installs: bool) -> (World, Arc<Mutex<Vec<String>>>) {
    let mut setup = Setup::new(|_, _| Vec::new());
    setup.machines = vec![linux("m1")];
    let claude = Arc::new(Mutex::new("2.1.318".to_string()));
    let codex = Arc::new(Mutex::new("0.118.0".to_string()));
    let ran = Arc::new(Mutex::new(Vec::new()));
    let (c, x) = (claude.clone(), codex.clone());
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains(PROBE)),
        move |_| {
            CommandResult::ok(&format!(
                "claude.path=/home/w/.local/bin/claude\nclaude.link=/home/w/.local/share/claude/versions/x\ncodex.path=/usr/local/bin/codex\ncodex.link=../lib/node_modules/@openai/codex/bin/codex.js\nagy.path=\nnotchling.path=\nclaude.version={} (Claude Code)\ncodex.version=codex-cli {}\n",
                lock(&c),
                lock(&x)
            ))
        },
    );
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("busy=")),
        |_| CommandResult::ok("busy=0\n"),
    );
    let r = ran.clone();
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("install")),
        move |call| {
            let text = call.argv.last().cloned().unwrap_or_default();
            if text.contains("claude") {
                lock(&r).push("claude".to_string());
                *lock(&claude) = "2.1.327".to_string();
            } else {
                lock(&r).push("codex".to_string());
                if codex_installs {
                    *lock(&codex) = "0.166.0".to_string();
                }
            }
            CommandResult::ok("")
        },
    );
    (setup.build(), ran)
}

#[test]
fn update_tools_runs_every_tool_in_order() {
    let (w, ran) = both_rig(true);
    w.engine.read_worker_tools("m1").unwrap();
    let out = w.click(
        "m1",
        PlanKind::UpdateTools(vec![
            mv(Tool::Claude, "2.1.327"),
            mv(Tool::Codex, "0.166.0"),
        ]),
        AdminAnswer::None,
    );
    assert!(out.ok, "{}", out.message);
    assert_eq!(lock(&ran).clone(), ["claude", "codex"]);
    assert!(out.message.contains("Claude Code on m1 is now 2.1.327"));
    assert!(out.message.contains("Codex on m1 is now 0.166.0"));
}

#[test]
fn update_tools_stops_at_the_first_failure_and_says_what_was_done() {
    let (w, ran) = both_rig(false);
    w.engine.read_worker_tools("m1").unwrap();
    let out = w.click(
        "m1",
        PlanKind::UpdateTools(vec![
            mv(Tool::Claude, "2.1.327"),
            mv(Tool::Codex, "0.166.0"),
        ]),
        AdminAnswer::None,
    );
    assert!(!out.ok);
    assert!(
        out.message.contains("Codex on m1 still reports 0.118.0"),
        "{}",
        out.message
    );
    assert!(
        out.message
            .contains("Claude Code on m1 is now 2.1.327 already updated"),
        "{}",
        out.message
    );
    assert_eq!(lock(&ran).clone(), ["claude", "codex"]);
}
