//! Runs the checks (Swift `SetupEngine`, the check half; the Fix half is [`super::engine_fix`]).
//!
//! - A **check** is read-only. [`SetupEngine::refresh`] batches a machine's remote probes into one
//!   ssh per phase (the cheap ones with 60 s, then the slow ones whose needs passed with 180 s);
//!   local probes run up to four at a time. A step whose need isn't passing says `waits for <title>`.
//! - Slow probes run on open, Check or a Fix (`include_slow`), and on their own every 10 minutes.
//! - **Secrets.** Every string that leaves the engine (a result, a log line, a Fix's message) goes
//!   through the machine's masks: its vault values as `[secret:<name>]`, the admin password as
//!   `[admin password]`. The password lives in one Fix's context; no field of the engine holds it.

use super::engine_plan::ShownRecord;
use super::github::GitHubHttp;
use super::masks::Masks;

use super::registry::{Machine, RegistryFiles};
use super::runner::Runner;
use super::shell;
use super::step::{
    CheckResult, Detected, FixContext, FixOutcome, MacFacts, Opener, SecretSource, SetupStep,
    StepContext, StepId,
};
use super::usage::SecretUsage;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

pub type StepList = Arc<dyn Fn(&Machine, &[Machine]) -> Vec<SetupStep> + Send + Sync>;
pub type UsageList = Arc<dyn Fn(&[Machine]) -> Vec<SecretUsage> + Send + Sync>;
pub type Deliver = Arc<dyn Fn(&SecretUsage, &FixContext) -> FixOutcome + Send + Sync>;
pub type ToggleScript = Arc<dyn Fn(bool, bool) -> String + Send + Sync>;
pub type OffersReplace = Arc<dyn Fn(Option<&CheckResult>) -> bool + Send + Sync>;

/// What the engine builds steps and deliveries from; [`super::catalog::standard`] is the real one.
#[derive(Clone)]
pub struct Catalog {
    pub steps: StepList,
    pub usages: UsageList,
    pub deliver: Deliver,
    /// `(linux, on)` → the Running switch's script.
    pub toggle_script: ToggleScript,
    pub offers_replace: OffersReplace,
}

/// Where the machines come from and go to.
pub trait MachineStore: Send + Sync {
    fn machines(&self) -> Vec<Machine>;
    /// `false`: not saved (the file can't be read and is never written over).
    fn save(&self, machines: &[Machine]) -> bool;
}

impl MachineStore for RegistryFiles {
    fn machines(&self) -> Vec<Machine> {
        self.all()
    }
    fn save(&self, machines: &[Machine]) -> bool {
        RegistryFiles::save(self, machines)
    }
}

/// Everything outside the engine, as parameters.
#[derive(Clone)]
pub struct Deps {
    pub runner: Arc<dyn Runner>,
    pub secrets: Arc<dyn SecretSource>,
    pub http: Arc<dyn GitHubHttp>,
    pub opener: Arc<dyn Opener>,
    pub store: Arc<dyn MachineStore>,
    pub mac: Arc<dyn Fn() -> MacFacts + Send + Sync>,
    /// Seconds since 1970.
    pub clock: Arc<dyn Fn() -> f64 + Send + Sync>,
    /// A private folder for files a Fix hands `gh`.
    pub scratch: PathBuf,
    /// `pending-updates.json`; `None` keeps the offers in memory only.
    pub offers_file: Option<PathBuf>,
    /// `audit.jsonl`: one line per Fix that ran (time, machine, step, outcome; never its output).
    pub audit: Option<PathBuf>,
}

pub const LOG_LIMIT: usize = 2000;
pub const CHEAP_TIMEOUT: u64 = 60;
pub const SLOW_TIMEOUT: u64 = 180;
pub const SLOW_EVERY: f64 = 10.0 * 60.0;
pub(super) const LOCAL_PARALLEL: usize = 4;
/// The most a probe's output may be (the `head -c` in the batch wrapper).
pub const PROBE_CAP: usize = 65536;
pub(super) const CONNECT_ERRORS: [&str; 4] = [
    "Could not resolve hostname",
    "timed out",
    "No route to host",
    "Network is unreachable",
];

#[derive(Default)]
pub(super) struct State {
    pub results: BTreeMap<String, BTreeMap<StepId, CheckResult>>,
    pub running: BTreeMap<String, StepId>,
    pub logs: BTreeMap<String, Vec<String>>,
    pub offers: BTreeMap<String, Vec<SecretUsage>>,
    pub save_refused: bool,
    pub last_slow: BTreeMap<String, f64>,
    pub facts: BTreeMap<String, Detected>,
    pub saved_state: BTreeMap<String, Machine>,
    pub expiry_sent: BTreeMap<String, String>,
    pub offer_revision: BTreeMap<String, u64>,
    pub refresh_started: BTreeMap<String, u64>,
    pub fixes_ended: BTreeMap<String, u64>,
    pub step_published: BTreeMap<String, BTreeMap<StepId, u64>>,
    /// Plans shown and not yet confirmed, by id.
    pub shown: BTreeMap<u64, ShownRecord>,
    pub revision: u64,
}

/// Runs the checks and the Fixes. Shared between threads; every method takes `&self`.
pub struct SetupEngine {
    pub(super) deps: Deps,
    pub(super) catalog: Catalog,
    pub(super) state: Arc<Mutex<State>>,
    pub(super) on_change: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

#[derive(Default)]
pub(super) struct Evaluation {
    pub results: BTreeMap<StepId, CheckResult>,
    pub detected: Detected,
    pub tried: bool,
    pub unreachable: bool,
    pub connect_error: bool,
    pub slow_ran: bool,
    pub carried: BTreeSet<StepId>,
}

pub(super) enum Batch {
    Unreachable(String, bool),
    Answers(BTreeMap<StepId, CheckResult>),
}

pub(super) fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `m` with the detected facts merged in. A path from a machine counts only when it is safe.
pub fn applying(d: &Detected, m: &Machine) -> Machine {
    let mut m = m.clone();
    if let Some(os) = d.os {
        m.os = Some(os);
    }
    if let Some(fv) = d.file_vault {
        m.file_vault = fv;
    }
    if let Some(a) = &d.abilities {
        let mut a = a.clone();
        if a.home.is_none() {
            a.home = m.abilities.as_ref().and_then(|x| x.home.clone());
        }
        m.abilities = Some(a);
    }
    if let Some(home) = d.home.as_ref().filter(|h| shell::is_safe_machine_path(h)) {
        let mut a = m.abilities.clone().unwrap_or_default();
        a.home = Some(home.clone());
        m.abilities = Some(a);
    }
    if let Some(job) = m.sd_worker_mut() {
        if let Some(p) = d.python.as_ref().filter(|p| shell::is_safe_machine_path(p)) {
            job.python = p.clone();
        }
        if let Some(p) = d
            .sd_path
            .as_ref()
            .filter(|p| shell::is_safe_machine_path(p))
        {
            job.sd_path = p.clone();
        }
        if let Some(day) = &d.token_expires {
            job.token_expires = Some(day.clone());
        }
    }
    m
}

impl SetupEngine {
    pub fn new(deps: Deps, catalog: Catalog) -> SetupEngine {
        let engine = SetupEngine {
            deps,
            catalog,
            state: Arc::new(Mutex::new(State::default())),
            on_change: Mutex::new(None),
        };
        engine.load_offers();
        engine
    }

    /// Called after anything shown changed (results, the log, a Fix starting or ending).
    pub fn set_on_change(&self, f: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.on_change.lock().unwrap_or_else(|e| e.into_inner()) = f;
    }

    pub(super) fn changed(&self) {
        lock(&self.state).revision += 1;
        let f = self
            .on_change
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(f) = f {
            f();
        }
    }

    // MARK: what the window reads

    pub fn revision(&self) -> u64 {
        lock(&self.state).revision
    }
    pub fn results(&self, machine: &str) -> BTreeMap<StepId, CheckResult> {
        lock(&self.state)
            .results
            .get(machine)
            .cloned()
            .unwrap_or_default()
    }
    pub fn running(&self, machine: &str) -> Option<StepId> {
        lock(&self.state).running.get(machine).cloned()
    }
    pub fn log(&self, machine: &str) -> Vec<String> {
        lock(&self.state)
            .logs
            .get(machine)
            .cloned()
            .unwrap_or_default()
    }
    /// Secret name → the machines' files waiting for its renewed value.
    pub fn update_offers(&self) -> BTreeMap<String, Vec<SecretUsage>> {
        lock(&self.state).offers.clone()
    }
    pub fn save_refused(&self) -> bool {
        lock(&self.state).save_refused
    }

    // MARK: machines and steps

    pub(super) fn current_machines(&self) -> Vec<Machine> {
        let list = self.deps.store.machines();
        let state = lock(&self.state);
        list.iter().map(|m| with_facts(&state, m)).collect()
    }

    pub(super) fn machine(&self, name: &str) -> Option<Machine> {
        self.current_machines().into_iter().find(|m| m.name == name)
    }

    /// `machine` with the facts the checks learned laid over it.
    pub fn overlaid(&self, m: &Machine) -> Machine {
        with_facts(&lock(&self.state), m)
    }

    pub fn steps(&self, m: &Machine) -> Vec<SetupStep> {
        let all = self.current_machines();
        (self.catalog.steps)(&self.overlaid(m), &all)
    }

    pub(super) fn step_list(&self, m: &Machine) -> Vec<SetupStep> {
        (self.catalog.steps)(m, &self.current_machines())
    }

    /// The vault values of the machine's usages, each hiding as `[secret:<name>]`.
    pub(super) fn masks(&self, m: &Machine, admin_password: Option<&str>) -> Masks {
        let mut names: BTreeSet<String> = (self.catalog.usages)(&self.current_machines())
            .into_iter()
            .filter(|u| u.machine == m.name)
            .map(|u| u.name)
            .collect();
        names.insert(m.claude_secret.clone());
        if let Some(job) = m.sd_worker().filter(|j| !j.issues_secret.is_empty()) {
            names.insert(job.issues_secret.clone());
        }
        let mut masks = Masks::new().with_admin(admin_password);
        for name in names {
            if let Some(v) = self.deps.secrets.value(&name) {
                masks.hide(&name, v.expose());
            }
        }
        masks
    }

    pub(super) fn context(&self, m: &Machine, masks: &Masks) -> StepContext {
        StepContext {
            machine: m.clone(),
            all: self.current_machines(),
            runner: self.deps.runner.clone(),
            secrets: self.deps.secrets.clone(),
            http: self.deps.http.clone(),
            opener: self.deps.opener.clone(),
            mac: (self.deps.mac)(),
            now: (self.deps.clock)(),
            scratch: self.deps.scratch.clone(),
            masks: masks.clone(),
        }
    }

    // MARK: checks

    /// Checks only, never a Fix. `names` `None` means every machine.
    pub fn refresh(&self, names: Option<&[String]>, include_slow: bool) {
        let targets: Vec<Machine> = self
            .current_machines()
            .into_iter()
            .filter(|m| names.is_none_or(|n| n.contains(&m.name)))
            .collect();
        if targets.is_empty() {
            return;
        }
        let masks: BTreeMap<String, Masks> = targets
            .iter()
            .map(|m| (m.name.clone(), self.masks(m, None)))
            .collect();
        let now = (self.deps.clock)();
        let mut began = BTreeMap::new();
        let mut slow_of = BTreeMap::new();
        {
            let mut s = lock(&self.state);
            for m in &targets {
                let n = s.refresh_started.entry(m.name.clone()).or_default();
                *n += 1;
                let ticket = *n;
                let fixes = s.fixes_ended.get(&m.name).copied().unwrap_or(0);
                began.insert(m.name.clone(), (ticket, fixes));
                let due = s
                    .last_slow
                    .get(&m.name)
                    .is_some_and(|t| now - t > SLOW_EVERY);
                slow_of.insert(m.name.clone(), include_slow || due);
            }
        }
        let mut evaluations: BTreeMap<String, Evaluation> = std::thread::scope(|scope| {
            let handles: Vec<_> = targets
                .iter()
                .map(|m| {
                    let mk = masks.get(&m.name).cloned().unwrap_or_default();
                    let slow = slow_of.get(&m.name).copied().unwrap_or(false);
                    scope.spawn(move || (m.name.clone(), self.evaluate(m, slow, true, &mk)))
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });

        let tried: Vec<&Evaluation> = evaluations.values().filter(|e| e.tried).collect();
        let offline = tried.len() >= 2 && tried.iter().all(|e| e.unreachable && e.connect_error);
        let registry: Vec<String> = self
            .deps
            .store
            .machines()
            .into_iter()
            .map(|m| m.name)
            .collect();
        let mut learned = false;
        for m in &targets {
            let Some(mut e) = evaluations.remove(&m.name) else {
                continue;
            };
            let Some(&(ticket, fixes)) = began.get(&m.name) else {
                continue;
            };
            let known: BTreeSet<StepId> = self.step_list(m).into_iter().map(|s| s.id).collect();
            let mk = masks.get(&m.name).cloned().unwrap_or_default();
            let mut s = lock(&self.state);
            // dropped whole: the machine was removed meanwhile, or a Fix ended since this began
            if !registry.contains(&m.name)
                || s.fixes_ended.get(&m.name).copied().unwrap_or(0) != fixes
            {
                continue;
            }
            if offline && e.results.contains_key(&StepId::REACHABLE) {
                e.results.insert(
                    StepId::REACHABLE,
                    CheckResult::cant_tell("no machine answered; this Mac may be offline"),
                );
            }
            let mut published: BTreeMap<StepId, CheckResult> = s
                .results
                .get(&m.name)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|(id, _)| known.contains(id))
                .collect();
            let mut tickets = s.step_published.get(&m.name).cloned().unwrap_or_default();
            for (id, r) in e.results {
                if e.carried.contains(&id) {
                    published.entry(id).or_insert_with(|| r.masked(&mk));
                    continue;
                }
                if tickets.get(&id).copied().unwrap_or(0) >= ticket {
                    continue;
                }
                tickets.insert(id.clone(), ticket);
                published.insert(id, r.masked(&mk));
            }
            s.step_published.insert(m.name.clone(), tickets);
            s.results.insert(m.name.clone(), published);
            if e.slow_ran {
                s.last_slow.insert(m.name.clone(), (self.deps.clock)());
            }
            if e.detected != Detected::default() {
                let old = s.facts.get(&m.name).cloned().unwrap_or_default();
                s.facts.insert(m.name.clone(), old.merged(&e.detected));
                learned = true;
            }
        }
        if learned {
            let names: BTreeSet<String> = targets.iter().map(|m| m.name.clone()).collect();
            self.save_facts(&names);
        }
        self.changed();
    }

    /// For the Add sheet: the machine steps of a draft, nothing saved.
    pub fn check_draft(&self, draft: &Machine) -> BTreeMap<StepId, CheckResult> {
        let mut m = draft.clone();
        m.jobs.clear();
        let masks = self.masks(&m, None);
        let e = self.evaluate(&m, false, false, &masks);
        e.results
            .into_iter()
            .map(|(k, v)| (k, v.masked(&masks)))
            .collect()
    }

    /// Writes the facts the checks learned into the registry, when they changed anything.
    fn save_facts(&self, names: &BTreeSet<String>) {
        let mut list = self.deps.store.machines();
        let mut changed = false;
        let mut expiries = Vec::new();
        {
            let mut s = lock(&self.state);
            for raw in list.iter_mut().filter(|m| names.contains(&m.name)) {
                let merged = with_facts(&s, raw);
                if let Some(job) = merged.sd_worker() {
                    if let Some(day) = &job.token_expires {
                        if !job.issues_secret.is_empty()
                            && s.expiry_sent.get(&raw.name) != Some(day)
                        {
                            s.expiry_sent.insert(raw.name.clone(), day.clone());
                            expiries.push((job.issues_secret.clone(), day.clone()));
                        }
                    }
                }
                if merged == *raw || s.saved_state.get(&raw.name) == Some(&merged) {
                    continue;
                }
                *raw = merged;
                changed = true;
            }
        }
        for (secret, day) in expiries {
            let _ = self.deps.secrets.set_expires(&secret, Some(&day));
        }
        if !changed {
            return;
        }
        let saved = self.deps.store.save(&list);
        let mut s = lock(&self.state);
        s.save_refused = !saved;
        if saved {
            for m in list.iter().filter(|m| names.contains(&m.name)) {
                s.saved_state.insert(m.name.clone(), m.clone());
                s.facts.remove(&m.name);
            }
        }
    }
}

pub(super) fn with_facts(s: &State, m: &Machine) -> Machine {
    match s.facts.get(&m.name) {
        Some(d) => applying(d, m),
        None => m.clone(),
    }
}
