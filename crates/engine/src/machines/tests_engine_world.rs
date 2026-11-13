use super::engine::{Catalog, Deps, MachineStore, SetupEngine};
use super::engine_plan::{AdminAnswer, PlanKind};
use super::github::NoGitHub;
use super::registry::{Machine, MachineOs};
use super::runner::{CommandResult, Recorded, ScriptedRunner};
use super::step::{
    Cadence, CheckResult, FixOutcome, FixPlan, HoldWhen, MacFacts, NoOpener, Probe, SecretSource,
    SetupStep, StepFix, StepId,
};
use crate::accounts::Secret;
use crate::vault::VaultEntry;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const T0: f64 = 1_800_000_000.0;

pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn mac(name: &str) -> Machine {
    let mut m = Machine::new(name, &format!("{name}.test"), "worker");
    m.os = Some(MachineOs::MacOs);
    m
}

pub fn linux(name: &str) -> Machine {
    let mut m = Machine::new(name, &format!("{name}.test"), "worker");
    m.os = Some(MachineOs::Linux);
    m
}

#[derive(Default)]
pub struct Store {
    pub list: Mutex<Vec<Machine>>,
    pub saves: Mutex<Vec<Vec<Machine>>>,
    pub refuse: Mutex<bool>,
}

impl MachineStore for Store {
    fn machines(&self) -> Vec<Machine> {
        lock(&self.list).clone()
    }
    fn save(&self, machines: &[Machine]) -> bool {
        lock(&self.saves).push(machines.to_vec());
        if *lock(&self.refuse) {
            return false;
        }
        *lock(&self.list) = machines.to_vec();
        true
    }
}

#[derive(Default)]
pub struct Secrets {
    pub values: Mutex<BTreeMap<String, String>>,
    pub reads: Mutex<BTreeMap<String, usize>>,
    pub expiries: Mutex<Vec<String>>,
}

impl Secrets {
    pub fn with(values: &[(&str, &str)]) -> Secrets {
        let s = Secrets::default();
        *lock(&s.values) = values
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        s
    }
}

impl SecretSource for Secrets {
    fn value(&self, name: &str) -> Option<Secret> {
        *lock(&self.reads).entry(name.into()).or_default() += 1;
        lock(&self.values).get(name).map(Secret::new)
    }
    fn entry(&self, _name: &str) -> Option<VaultEntry> {
        None
    }
    fn set_expires(&self, name: &str, day: Option<&str>) -> Result<(), String> {
        lock(&self.expiries).push(format!("{name}={}", day.unwrap_or("")));
        Ok(())
    }
}

pub fn batch_probes(script: &str) -> Option<(String, Vec<String>)> {
    if !script.contains("out=$( /bin/sh -c ") {
        return None;
    }
    let at = script.find("@@")?;
    let nonce: String = script[at + 2..].chars().take(16).collect();
    let q = "'\\''";
    let probes = script
        .split("out=$( /bin/sh -c ")
        .skip(1)
        .map(|seg| {
            let p = seg.split(" </dev/null").next().unwrap_or("");
            let p = p.strip_prefix(q).unwrap_or(p);
            p.strip_suffix(q).unwrap_or(p).to_string()
        })
        .collect();
    Some((nonce, probes))
}

pub fn is_batch(r: &Recorded) -> bool {
    r.argv
        .last()
        .is_some_and(|s| s.contains("out=$( /bin/sh -c "))
}

pub fn is_sudo(r: &Recorded) -> bool {
    r.argv.last().is_some_and(|s| s.contains("sudo -S"))
}

pub type Answers = Arc<Mutex<BTreeMap<String, (String, i32)>>>;

pub struct World {
    pub runner: Arc<ScriptedRunner>,
    pub store: Arc<Store>,
    pub secrets: Arc<Secrets>,
    pub clock: Arc<Mutex<f64>>,
    pub answers: Answers,
    pub engine: SetupEngine,
}

pub struct Setup {
    pub machines: Vec<Machine>,
    pub secrets: Secrets,
    pub steps: super::engine::StepList,
    pub usages: super::engine::UsageList,
    pub deliver: super::engine::Deliver,
    pub offers_data: Option<PathBuf>,
    pub audit: Option<PathBuf>,
    pub runner: ScriptedRunner,
    pub toggle: Option<super::engine::ToggleScript>,
}

impl Setup {
    pub fn new(
        steps: impl Fn(&Machine, &[Machine]) -> Vec<SetupStep> + Send + Sync + 'static,
    ) -> Setup {
        Setup {
            machines: vec![mac("m1")],
            secrets: Secrets::default(),
            steps: Arc::new(steps),
            usages: Arc::new(|_| Vec::new()),
            deliver: Arc::new(|_, _| FixOutcome::ok("")),
            offers_data: None,
            audit: None,
            runner: ScriptedRunner::new(),
            toggle: None,
        }
    }

    pub fn build(self) -> World {
        let answers: Answers = Arc::new(Mutex::new(BTreeMap::new()));
        let a = answers.clone();
        self.runner.on_call(
            |argv| {
                argv.last()
                    .is_some_and(|s| s.contains("out=$( /bin/sh -c "))
            },
            move |call| {
                let script = call.argv.last().cloned().unwrap_or_default();
                let Some((nonce, probes)) = batch_probes(&script) else {
                    return CommandResult::ok("");
                };
                let mut out = String::new();
                for (i, p) in probes.iter().enumerate() {
                    let (text, code) = lock(&a).get(p).cloned().unwrap_or(("ok".into(), 0));
                    out.push_str(&format!("{text}\n@@{nonce} {i} {code}@@\n"));
                }
                CommandResult::ok(&out)
            },
        );
        let runner = Arc::new(self.runner);
        let store = Arc::new(Store::default());
        *lock(&store.list) = self.machines;
        let secrets = Arc::new(self.secrets);
        let clock = Arc::new(Mutex::new(T0));
        let c = clock.clone();
        let deps = Deps {
            runner: runner.clone(),
            secrets: secrets.clone(),
            http: Arc::new(NoGitHub),
            opener: Arc::new(NoOpener),
            store: store.clone(),
            mac: Arc::new(MacFacts::default),
            clock: Arc::new(move || *lock(&c)),
            scratch: std::env::temp_dir(),
            offers_data: self.offers_data,
            audit: self.audit,
        };
        let catalog = Catalog {
            steps: self.steps,
            usages: self.usages,
            deliver: self.deliver,
            toggle_script: self.toggle.unwrap_or_else(|| {
                Arc::new(|linux, on| {
                    format!(
                        "toggle {} {}",
                        if linux { "linux" } else { "mac" },
                        if on { "on" } else { "off" }
                    )
                })
            }),
            offers_replace: Arc::new(|r| r.is_some_and(|r| r.state.is_pass())),
        };
        World {
            runner,
            store,
            secrets,
            clock,
            answers,
            engine: SetupEngine::new(deps, catalog),
        }
    }
}

impl World {
    pub fn answer(&self, probe: &str, text: &str, code: i32) {
        lock(&self.answers).insert(probe.into(), (text.into(), code));
    }
    pub fn set_clock(&self, t: f64) {
        *lock(&self.clock) = t;
    }
    pub fn refresh(&self, name: &str, slow: bool) {
        self.engine.refresh(Some(&[name.to_string()]), slow);
    }
    pub fn state(&self, machine: &str, id: &str) -> Option<super::step::CheckState> {
        self.engine
            .results(machine)
            .get(&StepId(id.to_string().into()))
            .map(|r| r.state.clone())
    }
    pub fn batches(&self) -> Vec<Recorded> {
        self.runner.calls().into_iter().filter(is_batch).collect()
    }
    pub fn click(&self, machine: &str, kind: PlanKind, admin: AdminAnswer) -> FixOutcome {
        let plan = match self.engine.plan(machine, kind) {
            Ok(p) => p,
            Err(e) => return FixOutcome::fail(e),
        };
        match self.engine.confirm(plan.id, admin) {
            Ok(c) => self.engine.run(c),
            Err(e) => FixOutcome::fail(e),
        }
    }
    pub fn fix(&self, machine: &str, id: &str, admin: AdminAnswer) -> FixOutcome {
        self.click(machine, PlanKind::Fix(sid(id)), admin)
    }
    pub fn log(&self, machine: &str) -> String {
        self.engine.log(machine).join("\n")
    }
}

pub fn sid(raw: &str) -> StepId {
    StepId(raw.to_string().into())
}

pub fn pw(s: &str) -> AdminAnswer {
    AdminAnswer::Password(Secret::new(s))
}

pub fn step(raw: &str) -> SetupStep {
    StepBuilder::new(raw).build()
}

pub struct StepBuilder {
    pub step: SetupStep,
}

impl StepBuilder {
    pub fn new(raw: &str) -> StepBuilder {
        StepBuilder {
            step: SetupStep::new(
                sid(raw),
                raw.to_uppercase(),
                Vec::new(),
                Cadence::EveryRefresh,
                Probe::remote(format!("echo probe-{raw}"), |out, _| {
                    if out.code == 0 {
                        CheckResult::pass(out.text.clone())
                    } else {
                        CheckResult::fail(out.text.clone())
                    }
                }),
                StepFix::HowTo("do it by hand".into()),
            ),
        }
    }
    pub fn title(mut self, t: &str) -> Self {
        self.step.title = t.into();
        self
    }
    pub fn needs(mut self, n: &[&str]) -> Self {
        self.step.needs = n.iter().map(|x| sid(x)).collect();
        self
    }
    pub fn slow(mut self) -> Self {
        self.step.cadence = Cadence::Slow;
        self
    }
    pub fn fix(mut self, f: StepFix) -> Self {
        self.step.fix = f;
        self
    }
    pub fn hold(mut self, h: HoldWhen) -> Self {
        self.step.hold_when = Some(h);
        self
    }
    pub fn parse(
        mut self,
        p: impl Fn(&super::step::RemoteOutput, &super::step::StepContext) -> CheckResult
            + Send
            + Sync
            + 'static,
    ) -> Self {
        let script = self.step.probe.script().unwrap_or_default().to_string();
        self.step.probe = Probe::remote(script, p);
        self
    }
    pub fn build(self) -> SetupStep {
        self.step
    }
}

pub fn counting_fix(
    counter: &Arc<Mutex<Vec<String>>>,
    name: &str,
    ok: bool,
    admin: bool,
) -> StepFix {
    let c = counter.clone();
    let n = name.to_string();
    StepFix::Run(FixPlan::new(format!("do {name}"), admin, move |_| {
        lock(&c).push(n.clone());
        if ok {
            FixOutcome::ok(format!("did {n}"))
        } else {
            FixOutcome::fail(format!("could not {n}"))
        }
    }))
}
