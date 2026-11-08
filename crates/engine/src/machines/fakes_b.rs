//! Test-only helpers for the repo and sd steps (Swift `MachineSetupFakes` in part). Nothing here
//! starts a process: the runner is a [`ScriptedRunner`], the hosts are `*.test`, every value is
//! obviously fake.

use super::github::NoGitHub;
use super::masks::Masks;
use super::registry::{Machine, MachineAbilities, MachineOs, MachineRepo, SdWorkerSettings};
use super::runner::{CommandResult, Recorded, ScriptedRunner};
use super::shell;
use super::step::{
    FixContext, FixPlan, MacFacts, NoOpener, NoSecrets, Probe, RemoteParse, SetupStep, StepContext,
    StepFix, StepId,
};
use crate::accounts::Secret;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub const REPO: &str = "dtthaison/lingeva-mono";

/// A 108-character token like a Claude OAuth token (obviously fake).
pub fn long_token() -> String {
    let alphabet: Vec<char> = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789-_"
        .chars()
        .collect();
    let body: String = (0..95)
        .map(|i| alphabet[(i * 7 + i / 3) % alphabet.len()])
        .collect();
    format!("sk-ant-oat01-{body}")
}

/// A scratch folder under the system temp dir, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "nl-machines-b-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch folder");
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn machine(name: &str, user: &str, home: &str, os: MachineOs, python: &str, kind: &str) -> Machine {
    let mut m = Machine::new(name, &format!("{name}.test"), user);
    m.os = Some(os);
    m.abilities = Some(MachineAbilities {
        home: Some(home.into()),
        ..Default::default()
    });
    m.set_sd_worker(Some(SdWorkerSettings {
        python: python.into(),
        sd_path: shell::sd_py(home),
        repos: vec![MachineRepo {
            github: REPO.into(),
            local_path: "/Users/test/lingeva-mono".into(),
            check_command: "true".into(),
            machine_path: None,
            verify_skill: None,
        }],
        kinds: vec![kind.into()],
        health_url: None,
        token_expires: None,
        issues_secret: format!("{name}-github-issues"),
        token_all_repos: None,
    }));
    m
}

/// A Mac that serves the iOS kind of the test repo.
pub fn mac(name: &str) -> Machine {
    machine(
        name,
        "tester",
        "/Users/tester",
        MachineOs::MacOs,
        "/opt/homebrew/bin/python3",
        "ios",
    )
}

/// A Linux server that serves the backend kind of the test repo.
pub fn linux(name: &str) -> Machine {
    machine(
        name,
        "agent",
        "/home/agent",
        MachineOs::Linux,
        "/usr/bin/python3",
        "backend",
    )
}

/// Everything a step or a Fix runs against: a scripted runner, a scratch folder, the machine.
pub struct Rig {
    pub runner: Arc<ScriptedRunner>,
    pub tmp: TempDir,
    pub machine: Machine,
    pub all: Vec<Machine>,
    pub mac: MacFacts,
    pub masks: Masks,
    pub password: Option<&'static str>,
}

impl Rig {
    pub fn new(machine: Machine) -> Rig {
        Rig {
            runner: Arc::new(ScriptedRunner::new()),
            tmp: TempDir::new("rig"),
            all: vec![machine.clone()],
            machine,
            mac: MacFacts::default(),
            masks: Masks::new(),
            password: Some("pw-admin-test"),
        }
    }

    pub fn with_mac(mut self, mac: MacFacts) -> Rig {
        self.mac = mac;
        self
    }

    pub fn with_all(mut self, all: Vec<Machine>) -> Rig {
        self.all = all;
        self
    }

    /// Hides `value` as `[secret:<name>]` in every text.
    pub fn hiding(mut self, name: &str, value: &str) -> Rig {
        self.masks.hide(name, value);
        self
    }

    pub fn without_password(mut self) -> Rig {
        self.password = None;
        self
    }

    pub fn step(&self) -> StepContext {
        StepContext {
            machine: self.machine.clone(),
            all: self.all.clone(),
            runner: self.runner.clone(),
            secrets: Arc::new(NoSecrets),
            http: Arc::new(NoGitHub),
            opener: Arc::new(NoOpener),
            mac: self.mac.clone(),
            now: 1_800_000_000.0,
            scratch: self.tmp.path().join("scratch"),
            masks: self.masks.clone(),
        }
    }

    pub fn fix(&self) -> FixContext {
        FixContext {
            step: self.step(),
            admin_password: self.password.map(Secret::new),
            log: Arc::new(|_, _| {}),
            masks: self.masks.clone().with_admin(self.password),
        }
    }

    /// The calls the runner saw, as argv.
    pub fn argvs(&self) -> Vec<Vec<String>> {
        self.runner.calls().into_iter().map(|c| c.argv).collect()
    }

    pub fn calls(&self) -> Vec<Recorded> {
        self.runner.calls()
    }
}

/// The script as the machine's shell sees it: ssh's argument is the script wrapped in one more
/// layer of quotes.
pub fn unquoted(arg: &str) -> String {
    arg.replace("'\\''", "'")
}

/// The last argument of an argv (an ssh call's script).
pub fn last(argv: &[String]) -> &str {
    argv.last().map(String::as_str).unwrap_or("")
}

pub fn find(steps: &[SetupStep], id: &StepId) -> SetupStep {
    steps
        .iter()
        .find(|s| s.id == *id)
        .unwrap_or_else(|| panic!("no step {}", id.raw()))
        .clone()
}

/// A remote probe's script and parse.
pub fn remote(step: &SetupStep) -> (String, RemoteParse) {
    match &step.probe {
        Probe::Remote { script, parse } => (script.clone(), parse.clone()),
        Probe::Local(_) => panic!("{} is not a remote probe", step.id.raw()),
    }
}

pub fn plan(step: &SetupStep) -> FixPlan {
    match &step.fix {
        StepFix::Run(plan) => plan.clone(),
        _ => panic!("{} has no Fix to run", step.id.raw()),
    }
}

pub fn ok(out: &str) -> CommandResult {
    CommandResult::ok(out)
}

pub fn result(code: i32, out: &str, err: &str) -> CommandResult {
    CommandResult::new(code, out, err)
}

/// Whether the last argument of a call contains `needle` (a script sent to a machine).
pub fn script_has(needle: &'static str) -> impl Fn(&[String]) -> bool + Send + Sync + 'static {
    move |a: &[String]| a.last().is_some_and(|s| s.contains(needle))
}

/// Panics when an 8-character piece of `value` shows in `text`.
pub fn assert_no_fragment(value: &str, text: &str) {
    let chars: Vec<char> = value.chars().collect();
    for i in 0..=(chars.len() - 8) {
        let piece: String = chars[i..i + 8].iter().collect();
        assert!(
            !text.contains(&piece),
            "a fragment of the value shows at {i}: {text}"
        );
    }
}

// MARK: repo test data

/// An ed25519 key body (obviously fake) and others.
pub const BODY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIMachineKeyForTestsOnly0123456789abcdefg";
pub const OTHER_BODY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIOtherKeyThatIsNotGitHubs0123456789abcdef";
pub const NEW_BODY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAINewKeyForReplaceTestsOnly0123456789abcdefgh";
/// GitHub's published ed25519 host key.
pub const GITHUB_HOST_BODY: &str =
    "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";

pub fn pub_line() -> String {
    format!("ssh-ed25519 {BODY} agents-m1")
}

pub const READ_ONLY_MSG: &str = "ERROR: The key you are authenticating with has been marked as read only.\nfatal: Could not read from remote repository.";

pub const WELCOME: &str = "Hi dtthaison/lingeva-mono! You've successfully authenticated, but GitHub does not provide shell access.";

/// `gh api --paginate repos/<REPO>/keys` for one key.
pub fn gh_keys(key: &str, read_only: Option<bool>) -> String {
    let flag = read_only
        .map(|r| format!(",\"read_only\":{r}"))
        .unwrap_or_default();
    format!("[{{\"id\":1,\"key\":\"{key}\",\"title\":\"agents-m1\"{flag}}}]")
}

pub fn is_list(a: &[String]) -> bool {
    a.iter().any(|s| s == &format!("repos/{REPO}/keys"))
}

// MARK: sd test data

use super::step::InstalledPlugin;

/// sd's own worker templates, as the Swift tests keep them.
pub fn templates_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../legacy/macos/Tests/NotchlingTests/Fixtures/worker-templates")
}

pub fn fixture_file(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../legacy/macos/Tests/NotchlingTests/Fixtures")
        .join(name)
}

/// A fake sd folder under `tmp` with the five templates copied in; its path.
pub fn sd_folder(tmp: &TempDir, name: &str) -> String {
    let dir = tmp.path().join(name).join("templates/worker");
    std::fs::create_dir_all(&dir).expect("templates folder");
    for entry in std::fs::read_dir(templates_fixture()).expect("fixtures") {
        let entry = entry.expect("entry");
        std::fs::copy(entry.path(), dir.join(entry.file_name())).expect("copy template");
    }
    tmp.path().join(name).to_string_lossy().into_owned()
}

pub fn plugin(version: &str, path: &str) -> InstalledPlugin {
    InstalledPlugin {
        version: version.into(),
        path: path.into(),
    }
}

/// This Mac with sd `sd_version` installed at `sd_path` and superpowers 7.0.0.
pub fn mac_facts(sd_version: &str, sd_path: &str) -> MacFacts {
    let mut f = MacFacts::default();
    f.plugins
        .insert("sd@system-design".into(), plugin(sd_version, sd_path));
    f.plugins.insert(
        "superpowers@claude-plugins-official".into(),
        plugin("7.0.0", "/Users/x/p/superpowers/7.0.0"),
    );
    f.marketplace_json
        .insert("system-design".into(), r#"{"name":"system-design"}"#.into());
    f
}
