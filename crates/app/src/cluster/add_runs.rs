use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use notchling_engine::accounts::Secret;
use notchling_engine::machines::steps_tailscale_linux::is_sign_in_link;
use notchling_locale as t;

use super::add::{AddFlow, Phase, Progress, RunJob};
use super::add_plan::{PlanRow, PlanView};
use super::model::ClusterModel;
use crate::machines::data::now_secs;

#[derive(Clone, PartialEq, Eq)]
pub struct SignInLink(String);

impl SignInLink {
    pub fn new(url: String) -> Option<SignInLink> {
        is_sign_in_link(&url).then_some(SignInLink(url))
    }

    pub fn url(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SignInLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(std::any::type_name::<SignInLink>()).finish()
    }
}

#[derive(Clone, Debug)]
pub struct Snap {
    pub name: String,
    pub host: String,
    pub user: String,
    pub phase: Phase,
    pub plan: Option<PlanView>,
    pub rows: Vec<PlanRow>,
    pub log: Vec<String>,
    pub sign_in: Option<SignInLink>,
}

impl Snap {
    pub fn phase(&self) -> &Phase {
        &self.phase
    }
}

fn active(phase: &Phase) -> bool {
    matches!(
        phase,
        Phase::Checking | Phase::Running { .. } | Phase::Joining { .. }
    )
}

#[derive(Clone)]
pub struct AddRuns {
    flows: Arc<Mutex<BTreeMap<String, AddFlow>>>,
    shown: Arc<Mutex<BTreeMap<String, Snap>>>,
    generation: Arc<AtomicU64>,
    latest: Arc<Mutex<ClusterModel>>,
}

impl Default for AddRuns {
    fn default() -> AddRuns {
        AddRuns::tied_to(Arc::new(AtomicU64::new(0)))
    }
}

impl AddRuns {
    pub fn tied_to(generation: Arc<AtomicU64>) -> AddRuns {
        AddRuns {
            flows: Arc::new(Mutex::new(BTreeMap::new())),
            shown: Arc::new(Mutex::new(BTreeMap::new())),
            generation,
            latest: Arc::new(Mutex::new(ClusterModel::default())),
        }
    }

    pub fn fixed(snaps: Vec<Snap>) -> AddRuns {
        let runs = AddRuns::default();
        *runs.shown.lock().unwrap_or_else(|e| e.into_inner()) = snaps
            .into_iter()
            .map(|snap| (snap.name.clone(), snap))
            .collect();
        runs
    }

    pub fn briefs(&self) -> Vec<(String, Phase)> {
        let mut out: Vec<(String, Phase)> = self
            .flows()
            .values()
            .map(|flow| (flow.name().to_owned(), flow.phase().clone()))
            .collect();
        let shown = self.shown.lock().unwrap_or_else(|e| e.into_inner());
        out.extend(shown.values().map(|s| (s.name.clone(), s.phase.clone())));
        out
    }

    fn flows(&self) -> MutexGuard<'_, BTreeMap<String, AddFlow>> {
        self.flows.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn bump(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn begin(&self, mut flow: AddFlow) -> bool {
        let name = flow.name().to_owned();
        let latest = self
            .latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        flow.seen(&latest, now_secs());
        {
            let mut flows = self.flows();
            if flows.get(&name).is_some_and(|f| active(f.phase())) {
                return false;
            }
            flows.insert(name.clone(), flow);
        }
        self.check(&name)
    }

    pub fn check(&self, name: &str) -> bool {
        let (probes, engine) = {
            let mut flows = self.flows();
            let Some(flow) = flows.get_mut(name) else {
                return false;
            };
            (flow.check(), flow.engine())
        };
        if probes.is_empty() {
            return false;
        }
        self.bump();
        let runs = self.clone();
        let name = name.to_owned();
        std::thread::Builder::new()
            .name("notchling-add-check".into())
            .spawn(move || {
                let mut probes = probes;
                while let Some(probe) = probes.pop() {
                    let reply = probe.run(&engine);
                    if let Some(flow) = runs.flows().get_mut(&name) {
                        probes.extend(flow.reply(reply));
                    }
                    runs.bump();
                }
            })
            .is_ok()
    }

    pub fn run(&self, name: &str, password: Option<Secret>) -> Result<(), String> {
        let (job, engine) = {
            let mut flows = self.flows();
            let flow = flows
                .get_mut(name)
                .ok_or_else(|| t::cluster::add_not_now().to_owned())?;
            flow.run(password)?;
            let job: RunJob = flow
                .take_job()
                .ok_or_else(|| t::cluster::add_not_now().to_owned())?;
            (job, flow.engine())
        };
        self.bump();
        let generation = self.generation.clone();
        engine.set_on_link(Some(Arc::new(move || {
            generation.fetch_add(1, Ordering::SeqCst);
        })));
        let runs = self.clone();
        let name = name.to_owned();
        std::thread::Builder::new()
            .name("notchling-add-run".into())
            .spawn(move || {
                let progress = |p: Progress| {
                    if let Some(flow) = runs.flows().get_mut(&name) {
                        flow.progress(p);
                    }
                    runs.bump();
                };
                let end = job.execute(&engine, &progress);
                if let Some(flow) = runs.flows().get_mut(&name) {
                    flow.finished(end, now_secs());
                }
                runs.bump();
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn get(&self, name: &str) -> Option<Snap> {
        let flows = self.flows();
        let Some(flow) = flows.get(name) else {
            return self
                .shown
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned();
        };
        let phase = flow.phase().clone();
        let sign_in = match phase {
            Phase::Running { .. } => flow
                .engine()
                .sign_in_link(flow.name())
                .and_then(SignInLink::new),
            _ => None,
        };
        let log = match phase {
            Phase::Running { .. } | Phase::Joining { .. } | Phase::Stopped(_) => flow.log(),
            _ => Vec::new(),
        };
        Some(Snap {
            name: flow.name().to_owned(),
            host: flow.host().to_owned(),
            user: flow.user().to_owned(),
            plan: flow.plan(),
            rows: flow.rows().to_vec(),
            phase,
            log,
            sign_in,
        })
    }

    pub fn set_can_be_master(&self, name: &str, on: bool) {
        if let Some(flow) = self.flows().get_mut(name) {
            flow.set_can_be_master(on);
        }
        self.bump();
    }

    pub fn set_acting(&self, name: &str, on: bool) {
        if let Some(flow) = self.flows().get_mut(name) {
            flow.set_acting(on);
        }
        self.bump();
    }

    pub fn dismiss(&self, name: &str) {
        let mut flows = self.flows();
        if flows.get(name).is_some_and(|f| !active(f.phase())) {
            flows.remove(name);
            drop(flows);
            self.bump();
        }
    }

    pub fn seen(&self, model: &ClusterModel) {
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = model.clone();
        let now = now_secs();
        let mut changed = false;
        for flow in self.flows().values_mut() {
            let before = flow.phase().clone();
            flow.seen(model, now);
            changed |= *flow.phase() != before;
        }
        if changed {
            self.bump();
        }
    }

    pub fn running(&self) -> Option<(String, usize, usize)> {
        self.flows().values().find_map(AddFlow::running_line)
    }

    pub fn lines(&self) -> Vec<String> {
        self.flows()
            .values()
            .filter_map(|flow| match flow.phase() {
                Phase::Running { step, of, .. } => {
                    Some(t::cluster::add_running(flow.name(), *step, *of))
                }
                Phase::Joining { .. } => Some(t::cluster::add_joining(flow.name())),
                _ => None,
            })
            .collect()
    }
}
