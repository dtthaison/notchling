use super::github::GitHubHttp;
use super::masks::Masks;
use super::registry::{Machine, MachineAbilities, MachineOs};
use super::runner::{Call, Channel, CommandResult, OutputSink, Runner};
use super::shell;
use crate::accounts::Secret;
use crate::vault::{VaultEntry, VaultRequest};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StepId(pub Cow<'static, str>);

impl StepId {
    pub const REACHABLE: StepId = StepId(Cow::Borrowed("reachable"));
    pub const WORKER_USER: StepId = StepId(Cow::Borrowed("worker-user"));
    pub const STAYS_AWAKE: StepId = StepId(Cow::Borrowed("stays-awake"));
    pub const FILEVAULT: StepId = StepId(Cow::Borrowed("filevault"));
    pub const BASE_TOOLS: StepId = StepId(Cow::Borrowed("base-tools"));
    pub const CLAUDE_CODE: StepId = StepId(Cow::Borrowed("claude-code"));
    pub const CLAUDE_LOGIN: StepId = StepId(Cow::Borrowed("claude-login"));
    pub const XCODE: StepId = StepId(Cow::Borrowed("xcode"));
    pub const PLUGINS: StepId = StepId(Cow::Borrowed("plugins"));
    pub const ISSUES_TOKEN: StepId = StepId(Cow::Borrowed("issues-token"));
    pub const TOKEN_SCOPE: StepId = StepId(Cow::Borrowed("token-scope"));
    pub const WORKER_JSON: StepId = StepId(Cow::Borrowed("worker-json"));
    pub const SERVICE: StepId = StepId(Cow::Borrowed("service"));
    pub const SD_CHECK: StepId = StepId(Cow::Borrowed("sd-check"));
    pub const RUNNING: StepId = StepId(Cow::Borrowed("running"));

    pub fn deploy_key(github: &str) -> StepId {
        StepId(Cow::Owned(format!("deploy-key:{github}")))
    }
    pub fn clone_repo(github: &str) -> StepId {
        StepId(Cow::Owned(format!("clone:{github}")))
    }
    pub fn push_refused(github: &str) -> StepId {
        StepId(Cow::Owned(format!("push-refused:{github}")))
    }

    pub fn raw(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckState {
    Pass(String),
    Fail(String),
    CantTell(String),
}

impl CheckState {
    pub fn text(&self) -> &str {
        match self {
            CheckState::Pass(s) | CheckState::Fail(s) | CheckState::CantTell(s) => s,
        }
    }
    pub fn is_pass(&self) -> bool {
        matches!(self, CheckState::Pass(_))
    }
    pub fn is_fail(&self) -> bool {
        matches!(self, CheckState::Fail(_))
    }
    fn map(&self, f: impl Fn(&str) -> String) -> CheckState {
        match self {
            CheckState::Pass(s) => CheckState::Pass(f(s)),
            CheckState::Fail(s) => CheckState::Fail(f(s)),
            CheckState::CantTell(s) => CheckState::CantTell(f(s)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubRow {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

impl SubRow {
    pub fn new(name: &str, ok: bool, detail: &str) -> SubRow {
        SubRow {
            name: name.into(),
            ok,
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Detected {
    pub os: Option<MachineOs>,
    pub home: Option<String>,
    pub file_vault: Option<bool>,
    pub python: Option<String>,
    pub sd_path: Option<String>,
    pub abilities: Option<MachineAbilities>,
    pub token_expires: Option<String>,
    pub sd_version: Option<String>,
}

impl Detected {
    pub fn merged(&self, new: &Detected) -> Detected {
        let mut d = self.clone();
        fn over<T: Clone>(slot: &mut Option<T>, v: &Option<T>) {
            if v.is_some() {
                slot.clone_from(v);
            }
        }
        over(&mut d.os, &new.os);
        over(&mut d.home, &new.home);
        over(&mut d.file_vault, &new.file_vault);
        over(&mut d.python, &new.python);
        over(&mut d.sd_path, &new.sd_path);
        over(&mut d.abilities, &new.abilities);
        over(&mut d.token_expires, &new.token_expires);
        over(&mut d.sd_version, &new.sd_version);
        d
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckResult {
    pub state: CheckState,
    pub rows: Vec<SubRow>,
    pub detected: Detected,
}

impl CheckResult {
    pub fn pass(s: impl Into<String>) -> CheckResult {
        CheckResult::of(CheckState::Pass(s.into()))
    }
    pub fn fail(s: impl Into<String>) -> CheckResult {
        CheckResult::of(CheckState::Fail(s.into()))
    }
    pub fn cant_tell(s: impl Into<String>) -> CheckResult {
        CheckResult::of(CheckState::CantTell(s.into()))
    }
    pub fn of(state: CheckState) -> CheckResult {
        CheckResult {
            state,
            rows: Vec::new(),
            detected: Detected::default(),
        }
    }
    pub fn rows(mut self, rows: Vec<SubRow>) -> CheckResult {
        self.rows = rows;
        self
    }
    pub fn detected(mut self, d: Detected) -> CheckResult {
        self.detected = d;
        self
    }

    pub fn masked(&self, masks: &Masks) -> CheckResult {
        CheckResult {
            state: self.state.map(|s| masks.apply(s)),
            rows: self
                .rows
                .iter()
                .map(|r| SubRow {
                    name: masks.apply(&r.name),
                    ok: r.ok,
                    detail: masks.apply(&r.detail),
                })
                .collect(),
            detected: self.detected.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteOutput {
    pub text: String,
    pub code: i32,
}

pub type RemoteParse = Arc<dyn Fn(&RemoteOutput, &StepContext) -> CheckResult + Send + Sync>;
pub type LocalProbe = Arc<dyn Fn(&StepContext) -> CheckResult + Send + Sync>;
pub type FixRun = Arc<dyn Fn(&FixContext) -> FixOutcome + Send + Sync>;
pub type HoldWhen = Arc<dyn Fn(&Detected) -> Option<String> + Send + Sync>;

#[derive(Clone)]
pub enum Probe {
    Remote { script: String, parse: RemoteParse },
    Local(LocalProbe),
}

impl Probe {
    pub fn remote(
        script: impl Into<String>,
        parse: impl Fn(&RemoteOutput, &StepContext) -> CheckResult + Send + Sync + 'static,
    ) -> Probe {
        Probe::Remote {
            script: script.into(),
            parse: Arc::new(parse),
        }
    }
    pub fn local(probe: impl Fn(&StepContext) -> CheckResult + Send + Sync + 'static) -> Probe {
        Probe::Local(Arc::new(probe))
    }
    pub fn script(&self) -> Option<&str> {
        match self {
            Probe::Remote { script, .. } => Some(script),
            Probe::Local(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
    EveryRefresh,
    Slow,
}

#[derive(Clone)]
pub struct FixPlan {
    pub display: String,
    pub needs_admin: bool,
    pub run: FixRun,
}

impl FixPlan {
    pub fn new(
        display: impl Into<String>,
        needs_admin: bool,
        run: impl Fn(&FixContext) -> FixOutcome + Send + Sync + 'static,
    ) -> FixPlan {
        FixPlan {
            display: display.into(),
            needs_admin,
            run: Arc::new(run),
        }
    }
}

#[derive(Clone)]
pub enum StepFix {
    Run(FixPlan),
    HowTo(String),
    Toggle { needs_admin: bool },
}

#[derive(Clone)]
pub struct SetupStep {
    pub id: StepId,
    pub title: String,
    pub needs: Vec<StepId>,
    pub cadence: Cadence,
    pub probe: Probe,
    pub fix: StepFix,
    pub replace: Option<FixPlan>,
    pub hold_when: Option<HoldWhen>,
}

impl SetupStep {
    pub fn new(
        id: StepId,
        title: impl Into<String>,
        needs: Vec<StepId>,
        cadence: Cadence,
        probe: Probe,
        fix: StepFix,
    ) -> SetupStep {
        SetupStep {
            id,
            title: title.into(),
            needs,
            cadence,
            probe,
            fix,
            replace: None,
            hold_when: None,
        }
    }

    pub fn replace_title(&self) -> String {
        self.title.replace("Deploy key", "Replace key")
    }

    pub fn plan(&self) -> Option<&FixPlan> {
        match &self.fix {
            StepFix::Run(p) => Some(p),
            _ => None,
        }
    }
}

pub trait SecretSource: Send + Sync {
    fn value(&self, name: &str) -> Option<Secret>;
    fn entry(&self, name: &str) -> Option<VaultEntry>;
    fn rename(&self, _old: &str, _new: &str) -> Result<(), String> {
        Err("the vault is off".into())
    }
    fn set_expires(&self, _name: &str, _day: Option<&str>) -> Result<(), String> {
        Err("the vault is off".into())
    }
}

pub struct NoSecrets;

impl SecretSource for NoSecrets {
    fn value(&self, _name: &str) -> Option<Secret> {
        None
    }
    fn entry(&self, _name: &str) -> Option<VaultEntry> {
        None
    }
}

pub trait Opener: Send + Sync {
    fn open_page(&self, url: &str);
    fn open_form(&self, request: &VaultRequest);
}

pub struct NoOpener;

impl Opener for NoOpener {
    fn open_page(&self, _url: &str) {}
    fn open_form(&self, _request: &VaultRequest) {}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub version: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MacFacts {
    pub plugins: BTreeMap<String, InstalledPlugin>,
    pub marketplace_json: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct StepContext {
    pub machine: Machine,
    pub all: Vec<Machine>,
    pub runner: Arc<dyn Runner>,
    pub secrets: Arc<dyn SecretSource>,
    pub http: Arc<dyn GitHubHttp>,
    pub opener: Arc<dyn Opener>,
    pub mac: MacFacts,
    pub now: f64,
    pub scratch: PathBuf,
    pub masks: Masks,
}

impl StepContext {
    pub fn mask(&self, text: &str) -> String {
        self.masks.apply(text)
    }
}

#[derive(Clone)]
pub struct FixContext {
    pub step: StepContext,
    pub admin_password: Option<Secret>,
    pub log: Arc<OutputSink>,
    pub masks: Masks,
}

impl FixContext {
    pub fn mask(&self, text: &str) -> String {
        self.masks.apply(text)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixOutcome {
    pub ok: bool,
    pub message: String,
}

impl FixOutcome {
    pub fn ok(message: impl Into<String>) -> FixOutcome {
        FixOutcome {
            ok: true,
            message: message.into(),
        }
    }
    pub fn fail(message: impl Into<String>) -> FixOutcome {
        FixOutcome {
            ok: false,
            message: message.into(),
        }
    }
}

pub mod run {
    use super::*;

    fn guarded(ctx: &FixContext, call: Call) -> CommandResult {
        let text = call.argv.join("\n")
            + &call
                .env
                .iter()
                .map(|(k, v)| format!("\n{k}={v}"))
                .collect::<String>();
        if ctx.masks.leaks_into(&text) {
            return CommandResult::refused("refused: a secret would have gone on a command line");
        }
        let log = ctx.log.clone();
        ctx.step
            .runner
            .run(&call.streaming(Some(Arc::new(move |c: Channel, t: &str| log(c, t)))))
    }

    pub fn script(ctx: &FixContext, s: &str, stdin: Option<Vec<u8>>, secs: u64) -> CommandResult {
        let mut call = Call::secs(shell::ssh_script(&ctx.step.machine, s), secs);
        call.stdin = stdin;
        guarded(ctx, call)
    }

    pub fn admin(ctx: &FixContext, s: &str, secs: u64) -> CommandResult {
        let m = &ctx.step.machine;
        if let Some(why) = shell::admin_refusal(m) {
            return CommandResult::refused(why);
        }
        let as_root = m.admin() == super::super::registry::AdminAccess::RootSsh;
        let mut call = Call::secs(shell::ssh_admin(m, s, as_root), secs);
        if !as_root {
            let Some(pw) = &ctx.admin_password else {
                return CommandResult::refused("needs the admin password");
            };
            call.stdin = Some(format!("{}\n", pw.expose()).into_bytes());
        }
        guarded(ctx, call)
    }

    pub fn local(
        ctx: &FixContext,
        argv: Vec<String>,
        stdin: Option<Vec<u8>>,
        secs: u64,
    ) -> CommandResult {
        let mut call = Call::secs(argv, secs);
        call.stdin = stdin;
        guarded(ctx, call)
    }

    pub fn local_env(
        ctx: &FixContext,
        argv: Vec<String>,
        env: &[(&str, &str)],
        timeout: Duration,
    ) -> CommandResult {
        let mut call = Call::new(argv, timeout);
        for (k, v) in env {
            call = call.env(k, v);
        }
        guarded(ctx, call)
    }
}
