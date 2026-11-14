use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use notchling_engine::machines::actions::MorningActions;
use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::forms::{
    job_problems, job_settings, machine_problems, migrated_note, secret_row, unreadable_note,
    JobDraft,
};
use notchling_engine::machines::morning::{self, GhIssue, MorningItem};
use notchling_engine::machines::poller::{LastReport, MachinePoller};
use notchling_engine::machines::registry::{
    backup_path, write_lock, AdminAccess, Machine, MachineOs, RegistryFiles, RegistryState,
};
use notchling_engine::machines::runner::Runner;
use notchling_engine::machines::shell::is_safe_machine_path;
use notchling_engine::machines::status::WorkerStatus;
use notchling_engine::machines::step::StepId;
use notchling_engine::machines::steps_secret::usages;
use notchling_engine::vault::entry::VaultEntry;
use notchling_engine::vault::expiry::today_local;

use super::forms::{AddForm, JobForm};
pub use super::model_sheet::AdminPassword;
use super::pieces::{DraftCheck, FixLine, FixPrompt, MachineChip, RunningSwitch, SecretRow};
pub use super::runs::{AdminAsks, PendingAdmin, Run, RunDone};
use super::versions_feed::MacVersions;

pub trait Registry: Send + Sync {
    fn load(&self) -> RegistryState;
    fn all(&self) -> Vec<Machine> {
        self.load().machines()
    }
    fn save(&self, machines: &[Machine]) -> bool;
    fn path(&self) -> String;
    fn backup(&self) -> String;
}

impl Registry for RegistryFiles {
    fn load(&self) -> RegistryState {
        RegistryFiles::load(self)
    }
    fn save(&self, machines: &[Machine]) -> bool {
        RegistryFiles::save(self, machines)
    }
    fn path(&self) -> String {
        self.source().display().to_string()
    }
    fn backup(&self) -> String {
        backup_path(&self.old_own).display().to_string()
    }
}

pub trait VaultList: Send + Sync {
    fn entries(&self) -> Vec<VaultEntry>;
    fn rename(&self, old: &str, new: &str) -> Result<(), String>;
    fn renew(&self, name: &str, url: Option<&str>) -> Result<(), String>;
    fn stamp(&self) -> Option<VaultStamp> {
        None
    }
}

pub type VaultStamp = (std::time::SystemTime, u64);

#[derive(Clone)]
pub struct Parts {
    pub engine: Arc<SetupEngine>,
    pub poller: Arc<MachinePoller>,
    pub actions: Arc<MorningActions>,
    pub registry: Arc<dyn Registry>,
    pub vault: Arc<dyn VaultList>,
    pub runner: Arc<dyn Runner>,
    pub clock: Arc<dyn Fn() -> f64 + Send + Sync>,
    pub off: Option<&'static str>,
    pub versions: Arc<dyn MacVersions>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Machines,
    Jobs,
}

impl Tab {
    pub const ALL: [Tab; 2] = [Tab::Machines, Tab::Jobs];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Machines => "Machines",
            Tab::Jobs => "Jobs",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Tab::Machines => "machines",
            Tab::Jobs => "jobs",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sheet {
    Fix(FixPrompt),
    Add(AddForm),
    Job(JobForm),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueFetch {
    pub generation: u64,
    pub repos: Vec<String>,
}

impl IssueFetch {
    pub fn run(&self, runner: &dyn Runner) -> (BTreeMap<String, Vec<GhIssue>>, BTreeSet<String>) {
        morning::fetch_issues_checked(&self.repos, runner)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffSheet {
    pub id: String,
    pub title: String,
    pub text: Option<String>,
}

pub struct MachinesModel {
    pub parts: Parts,
    pub asks: AdminAsks,
    pub(super) pending: Option<PendingAdmin>,
    pub tab: Tab,
    pub registry: RegistryState,
    registry_path: String,
    pub machines: Vec<Machine>,
    pub vault_entries: Vec<VaultEntry>,
    pub(super) vault_stamp: Option<VaultStamp>,
    pub(super) vault_seen: BTreeMap<String, String>,
    pub migration_note: Option<String>,
    pub rename_note: Option<String>,
    pub(super) renaming: bool,
    pub(super) checking: bool,
    pub registry_problem: Option<String>,
    pub notice: Option<String>,
    pub outcomes: BTreeMap<String, FixLine>,
    pub update_notes: BTreeMap<String, String>,
    pub(super) sheet: Option<Sheet>,
    pub(super) password: AdminPassword,
    pub items: Vec<MorningItem>,
    pub done: BTreeMap<String, String>,
    pub github_note: Option<String>,
    issues: BTreeMap<String, Vec<GhIssue>>,
    generation: u64,
    pub diff: Option<DiffSheet>,
}

impl MachinesModel {
    pub fn new(parts: Parts) -> MachinesModel {
        let mut model = MachinesModel {
            parts,
            asks: AdminAsks::default(),
            pending: None,
            tab: Tab::Machines,
            registry: RegistryState::Missing,
            registry_path: String::new(),
            machines: Vec::new(),
            vault_entries: Vec::new(),
            vault_stamp: None,
            vault_seen: BTreeMap::new(),
            migration_note: None,
            rename_note: None,
            renaming: false,
            checking: false,
            registry_problem: None,
            notice: None,
            outcomes: BTreeMap::new(),
            update_notes: BTreeMap::new(),
            sheet: None,
            password: AdminPassword::default(),
            items: Vec::new(),
            done: BTreeMap::new(),
            github_note: None,
            issues: BTreeMap::new(),
            generation: 0,
            diff: None,
        };
        model.refresh_machines();
        model.refresh_vault();
        model.rebase_vault();
        model
    }

    pub fn engine(&self) -> &SetupEngine {
        &self.parts.engine
    }

    fn refresh_machines(&mut self) {
        self.registry = self.parts.registry.load();
        self.registry_path = self.parts.registry.path();
        match &self.registry {
            RegistryState::Loaded { machines, migrated } => {
                self.machines = machines.clone();
                if *migrated {
                    self.migration_note = Some(migrated_note(&self.parts.registry.backup()));
                }
            }
            RegistryState::Missing | RegistryState::Unreadable(_) => self.machines.clear(),
        }
        self.rebuild();
    }

    pub(super) fn refresh_vault(&mut self) {
        self.vault_entries = self.parts.vault.entries();
    }

    pub fn registry_path(&self) -> String {
        self.registry_path.clone()
    }

    pub fn can_add(&self) -> bool {
        !matches!(self.registry, RegistryState::Unreadable(_))
    }

    pub fn off_reason(&self) -> Option<&'static str> {
        self.parts.off
    }

    pub fn can_act(&self) -> bool {
        self.parts.off.is_none()
    }

    pub fn add_block(&self) -> Option<String> {
        self.unreadable_note()
            .or_else(|| self.off_reason().map(str::to_owned))
    }

    pub fn unreadable_note(&self) -> Option<String> {
        match &self.registry {
            RegistryState::Unreadable(reason) => Some(unreadable_note(reason)),
            _ => None,
        }
    }

    pub fn dismiss_migration_note(&mut self) {
        self.migration_note = None;
    }

    pub fn reload(&mut self) -> Option<IssueFetch> {
        self.refresh_machines();
        self.refresh_vault();
        self.generation += 1;
        let repos: Vec<String> = self
            .machines
            .iter()
            .flat_map(|m| m.repos().iter().map(|r| r.github.clone()))
            .collect();
        (!repos.is_empty()).then_some(IssueFetch {
            generation: self.generation,
            repos,
        })
    }

    pub fn issues_fetched(
        &mut self,
        generation: u64,
        (fetched, failed): (BTreeMap<String, Vec<GhIssue>>, BTreeSet<String>),
    ) {
        if generation != self.generation {
            return;
        }
        self.issues = morning::merged(&self.issues, &fetched, &failed);
        self.github_note = (!failed.is_empty()).then(|| {
            format!(
                "Couldn't refresh from GitHub ({}). Titles and notes may be out of date.",
                failed.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        });
        self.rebuild();
    }

    fn rebuild(&mut self) {
        let mut reports = self.parts.poller.reports();
        for (name, report) in reports.iter_mut() {
            if report.status.is_none() {
                if let Some(old) = self
                    .parts
                    .poller
                    .previous_of(name)
                    .filter(|o| o.status.is_some())
                {
                    *report = old;
                }
            }
        }
        self.items = morning::build(&reports, &self.issues);
        let ids: BTreeSet<&str> = self.items.iter().map(|i| i.id.as_str()).collect();
        self.done.retain(|k, _| ids.contains(k.as_str()));
        self.parts.actions.prune_outcomes(&self.items);
    }

    pub fn chip(&self, machine: &Machine) -> MachineChip {
        let report = self.parts.poller.report(&machine.name);
        let previous = self.parts.poller.previous_of(&machine.name);
        MachineChip::make(
            machine,
            report.as_ref(),
            previous.as_ref(),
            (self.parts.clock)(),
        )
    }

    pub fn token_warnings(&self) -> Vec<String> {
        self.machines
            .iter()
            .filter_map(|m| self.chip(m).token_warning)
            .collect()
    }

    pub fn status_for(&self, machine: &Machine) -> Option<WorkerStatus> {
        let last = self
            .parts
            .poller
            .report(&machine.name)
            .and_then(|r| r.status);
        last.or_else(|| {
            self.parts
                .poller
                .previous_of(&machine.name)
                .and_then(|r| r.status)
        })
    }

    pub fn report_for(&self, machine: &Machine) -> Option<LastReport> {
        self.parts.poller.report(&machine.name)
    }

    pub fn machine(&self, name: &str) -> Option<Machine> {
        self.machines
            .iter()
            .find(|m| m.name == name)
            .map(|m| self.parts.engine.overlaid(m))
    }

    pub fn names(&self) -> Vec<String> {
        self.machines.iter().map(|m| m.name.clone()).collect()
    }

    pub fn running_switch(&self, machine: &Machine) -> RunningSwitch {
        let results = self.parts.engine.results(&machine.name);
        RunningSwitch::make(
            results.get(&StepId::RUNNING).map(|r| &r.state),
            self.parts.engine.turn_on_blocker(&machine.name).as_deref(),
            self.parts.engine.running(&machine.name).is_some(),
        )
    }

    pub fn secret_rows(&self, machine: &Machine) -> Vec<SecretRow> {
        let results = self.parts.engine.results(&machine.name);
        let today = today_local((self.parts.clock)());
        usages(std::slice::from_ref(machine))
            .into_iter()
            .filter(|u| u.machine == machine.name)
            .map(|usage| {
                let step = if usage.name == machine.claude_secret {
                    StepId::CLAUDE_LOGIN
                } else {
                    StepId::ISSUES_TOKEN
                };
                let entry = self
                    .vault_entries
                    .iter()
                    .find(|e| e.id() == usage.name.to_lowercase());
                SecretRow {
                    text: secret_row(&usage, entry, results.get(&step), today),
                    can_send: entry.is_some(),
                    usage,
                }
            })
            .collect()
    }

    fn commit(&mut self, change: impl FnOnce(&mut Vec<Machine>) -> bool) -> Result<(), String> {
        let saved = {
            let _writing = write_lock();
            let mut list = match self.parts.registry.load() {
                RegistryState::Loaded { machines, .. } => machines,
                RegistryState::Missing => Vec::new(),
                RegistryState::Unreadable(_) => {
                    return Err("machines.json can't be read, so nothing can be changed".into())
                }
            };
            if !change(&mut list) {
                return Err("no such machine".into());
            }
            self.parts.registry.save(&list)
        };
        if !saved {
            self.registry_problem = Some("Couldn't save machines.json".into());
            return Err("Couldn't save machines.json".into());
        }
        self.registry_problem = None;
        self.refresh_machines();
        Ok(())
    }

    pub fn set_admin(&mut self, machine: &str, access: AdminAccess) {
        let _ = self.commit(|list| match list.iter_mut().find(|m| m.name == machine) {
            Some(m) => {
                m.set_admin(access);
                true
            }
            None => false,
        });
    }

    pub fn remove(&mut self, machine: &str) {
        let removed = self.commit(|list| {
            let before = list.len();
            list.retain(|m| m.name != machine);
            list.len() != before
        });
        self.outcomes.remove(machine);
        if removed.is_ok() {
            self.parts.engine.forget(machine);
        }
    }

    pub fn save_job(&mut self, draft: &JobDraft) -> Result<(), String> {
        if let Some(first) = job_problems(draft).first() {
            return Err(first.clone());
        }
        self.commit(
            |list| match list.iter_mut().find(|m| m.name == draft.machine) {
                Some(m) => {
                    let settings = job_settings(draft, m.sd_worker());
                    m.set_sd_worker(Some(settings));
                    true
                }
                None => false,
            },
        )
    }

    pub fn draft_check(
        engine: &SetupEngine,
        name: &str,
        host: &str,
        user: &str,
        existing: &[String],
    ) -> DraftCheck {
        let inputs = vec![name.to_owned(), host.to_owned(), user.to_owned()];
        let problems = machine_problems(name, host, user, existing);
        if !problems.is_empty() {
            return DraftCheck::refused(problems, &inputs);
        }
        let results = engine.check_draft(&Machine::new(name, host, user));
        DraftCheck::from_results(&results, &inputs)
    }

    pub fn add(
        &mut self,
        name: &str,
        host: &str,
        user: &str,
        check: &DraftCheck,
    ) -> Result<(), String> {
        if !self.can_add() {
            return Err("machines.json can't be read, so nothing can be added".into());
        }
        if let Some(first) = machine_problems(name, host, user, &self.names()).first() {
            return Err(first.clone());
        }
        if !check.reachable {
            return Err("Check Reachable first".into());
        }
        if check.inputs != [name, host, user] {
            return Err("Check again; the details changed".into());
        }
        let mut machine = Machine::new(name, host, user);
        machine.os = check.detected.os;
        if machine.os == Some(MachineOs::Linux) {
            machine.set_admin(AdminAccess::RootSsh);
        }
        if let Some(fv) = check.detected.file_vault {
            machine.file_vault = fv;
        }
        if let Some(mut abilities) = check.detected.abilities.clone() {
            if let Some(home) = check
                .detected
                .home
                .as_ref()
                .filter(|h| is_safe_machine_path(h))
            {
                abilities.home = Some(home.clone());
            }
            machine.abilities = Some(abilities);
        }
        self.commit(|list| {
            if list.iter().any(|m| m.name == name) {
                return false;
            }
            list.push(machine);
            true
        })
    }

    pub fn may_act(&self, item: &MorningItem) -> bool {
        !self.parts.actions.is_running(&item.id) && !self.done.contains_key(&item.id)
    }

    pub fn acted(&mut self, item_id: &str, success: Option<&str>) {
        if let Some(text) = success {
            if self.parts.actions.last_error(item_id).is_none() {
                self.done.insert(item_id.to_owned(), text.to_owned());
            }
        }
        self.rebuild();
    }

    pub fn show_diff(&mut self, item: &MorningItem) {
        self.diff = Some(DiffSheet {
            id: item.id.clone(),
            title: format!("{} #{}", item.repo, item.issue),
            text: None,
        });
    }

    pub fn diff_loaded(&mut self, id: &str, text: String) {
        if let Some(diff) = self.diff.as_mut().filter(|d| d.id == id) {
            diff.text = Some(text);
        }
    }
}
