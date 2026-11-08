//! The window's model over a world in memory: a registry that saves to a list, a vault of names,
//! and the engine's `Refusing` or `ScriptedRunner`, so no test starts ssh, `gh` or `sudo`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use notchling_engine::machines::actions::MorningActions;
use notchling_engine::machines::catalog;
use notchling_engine::machines::engine::{Deps, MachineStore, SetupEngine};
use notchling_engine::machines::engine_plan::{AdminAnswer, AdminNeed, ApprovedStep, PlanKind};
use notchling_engine::machines::forms::JobDraft;
use notchling_engine::machines::github::NoGitHub;
use notchling_engine::machines::poller::MachinePoller;
use notchling_engine::machines::registry::{AdminAccess, Machine, MachineOs, RegistryState};
use notchling_engine::machines::runner::{Refusing, Runner, ScriptedRunner, OFF};
use notchling_engine::machines::step::{Detected, MacFacts, NoOpener, NoSecrets, StepId};
use notchling_engine::vault::entry::{Kind, VaultEntry};

use super::model::{MachinesModel, Parts, Registry, Sheet, VaultList};
use super::pieces::DraftCheck;

pub(super) const T0: f64 = 1_800_000_000.0;

pub(super) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
pub(super) struct MemRegistry {
    pub(super) list: Mutex<Vec<Machine>>,
    pub(super) unreadable: Mutex<Option<String>>,
    pub(super) saves: Mutex<usize>,
}

impl Registry for MemRegistry {
    fn load(&self) -> RegistryState {
        if let Some(why) = lock(&self.unreadable).clone() {
            return RegistryState::Unreadable(why);
        }
        RegistryState::Loaded {
            machines: lock(&self.list).clone(),
            migrated: false,
        }
    }
    fn save(&self, machines: &[Machine]) -> bool {
        if lock(&self.unreadable).is_some() {
            return false;
        }
        *lock(&self.saves) += 1;
        *lock(&self.list) = machines.to_vec();
        true
    }
    fn path(&self) -> String {
        "/data/machines.json".into()
    }
    fn backup(&self) -> String {
        "/data/machines.json.v1-backup".into()
    }
}

impl MachineStore for MemRegistry {
    fn machines(&self) -> Vec<Machine> {
        Registry::all(self)
    }
    fn save(&self, machines: &[Machine]) -> bool {
        Registry::save(self, machines)
    }
}

#[derive(Default)]
pub(super) struct FakeVault {
    pub(super) entries: Mutex<Vec<VaultEntry>>,
    pub(super) renames: Mutex<Vec<(String, String)>>,
    pub(super) renewed: Mutex<Vec<String>>,
}

impl VaultList for FakeVault {
    fn entries(&self) -> Vec<VaultEntry> {
        lock(&self.entries).clone()
    }
    fn rename(&self, old: &str, new: &str) -> Result<(), String> {
        lock(&self.renames).push((old.into(), new.into()));
        let mut entries = lock(&self.entries);
        match entries.iter_mut().find(|e| e.name == old) {
            Some(e) => {
                e.name = new.into();
                Ok(())
            }
            None => Err(format!("no {old}")),
        }
    }
    fn stamp(&self) -> Option<super::model::VaultStamp> {
        let n = lock(&self.renames).len() as u64;
        Some((std::time::UNIX_EPOCH + std::time::Duration::from_secs(n), 0))
    }
    fn renew(&self, name: &str, url: Option<&str>) -> Result<(), String> {
        lock(&self.renewed).push(name.into());
        url.map(|_| ()).ok_or("no page".into())
    }
}

pub(super) struct World {
    pub(super) registry: Arc<MemRegistry>,
    pub(super) vault: Arc<FakeVault>,
    pub(super) runner: Arc<ScriptedRunner>,
    pub(super) model: MachinesModel,
}

pub(super) fn mac(name: &str) -> Machine {
    let mut m = Machine::new(name, &format!("{name}.test"), "worker");
    m.os = Some(MachineOs::MacOs);
    m
}

pub(super) fn with_job(mut m: Machine) -> Machine {
    let mut job = notchling_engine::machines::registry::SdWorkerSettings::new(&m.name);
    job.sd_path = "/Users/worker/sd/sd.py".into();
    job.repos
        .push(notchling_engine::machines::registry::MachineRepo {
            github: "me/app".into(),
            local_path: "/Users/me/code/app".into(),
            check_command: "just check".into(),
            machine_path: None,
            verify_skill: None,
        });
    m.set_sd_worker(Some(job));
    m
}

pub(super) fn world_with(
    machines: Vec<Machine>,
    runner: Arc<dyn Runner>,
    scripted: Arc<ScriptedRunner>,
) -> World {
    let registry = Arc::new(MemRegistry::default());
    *lock(&registry.list) = machines;
    let vault = Arc::new(FakeVault::default());
    let store: Arc<dyn MachineStore> = registry.clone();
    let deps = Deps {
        runner: runner.clone(),
        secrets: Arc::new(NoSecrets),
        http: Arc::new(NoGitHub),
        opener: Arc::new(NoOpener),
        store,
        mac: Arc::new(MacFacts::default),
        clock: Arc::new(|| T0),
        scratch: std::env::temp_dir().join("notchling-machines-model-tests"),
        offers_file: None,
        audit: None,
    };
    let engine = Arc::new(SetupEngine::new(deps, catalog::standard()));
    let for_poller = registry.clone();
    let poller = Arc::new(MachinePoller::new(
        runner.clone(),
        move || Registry::all(&*for_poller),
        || T0,
    ));
    let for_actions = registry.clone();
    let actions = Arc::new(MorningActions::new(
        runner.clone(),
        move || Registry::all(&*for_actions),
        std::env::temp_dir().join("notchling-machines-model-tests-cache"),
    ));
    let parts = Parts {
        engine,
        poller,
        actions,
        registry: registry.clone(),
        vault: vault.clone(),
        runner,
        clock: Arc::new(|| T0),
        off: Some(OFF),
    };
    World {
        registry,
        vault,
        runner: scripted,
        model: MachinesModel::new(parts),
    }
}

/// A world whose runner refuses everything (the tool before handover).
pub(super) fn off_world(machines: Vec<Machine>) -> World {
    world_with(
        machines,
        Arc::new(Refusing(OFF)),
        Arc::new(ScriptedRunner::new()),
    )
}

/// A world whose runner records every call and answers each with an empty success.
pub(super) fn scripted_world(machines: Vec<Machine>) -> World {
    let scripted = Arc::new(ScriptedRunner::new());
    world_with(machines, scripted.clone(), scripted)
}

pub(super) fn fix_sheet(model: &MachinesModel) -> &super::pieces::FixPrompt {
    match &model.sheet {
        Some(Sheet::Fix(p)) => p,
        other => panic!("no Fix sheet: {other:?}"),
    }
}

#[test]
fn a_missing_file_lists_nothing_and_allows_add_an_unreadable_one_does_not() {
    let w = off_world(vec![]);
    assert!(w.model.machines.is_empty() && w.model.can_add());
    assert_eq!(w.model.unreadable_note(), None);
    assert_eq!(w.model.registry_path(), "/data/machines.json");

    let w = off_world(vec![mac("m1")]);
    *lock(&w.registry.unreadable) = Some("not JSON".into());
    let mut model = w.model;
    assert!(model.reload().is_none());
    assert!(!model.can_add());
    assert!(model.unreadable_note().unwrap().contains("not JSON"));
    assert!(model.machines.is_empty());
    let check = DraftCheck {
        reachable: true,
        inputs: vec!["m2".into(), "m2.test".into(), "w".into()],
        ..DraftCheck::default()
    };
    assert!(model.add("m2", "m2.test", "w", &check).is_err());
    assert_eq!(
        *lock(&w.registry.saves),
        0,
        "nothing written over an unreadable file"
    );
}

#[test]
fn add_needs_a_reachable_check_for_the_same_details_and_sets_linux_to_root() {
    let mut w = off_world(vec![mac("m1")]);
    let inputs = vec![
        "vps".to_string(),
        "vps.test".to_string(),
        "agent".to_string(),
    ];
    let unreached = DraftCheck::refused(vec![], &inputs);
    assert_eq!(
        w.model.add("vps", "vps.test", "agent", &unreached),
        Err("Check Reachable first".into())
    );
    let detected = Detected {
        os: Some(MachineOs::Linux),
        home: Some("/home/agent".into()),
        abilities: Some(Default::default()),
        ..Detected::default()
    };
    let check = DraftCheck {
        reachable: true,
        inputs: inputs.clone(),
        detected,
        ..DraftCheck::default()
    };
    assert_eq!(
        w.model.add("vps", "other.test", "agent", &check),
        Err("Check again; the details changed".into())
    );
    assert_eq!(
        w.model.add("m1", "vps.test", "agent", &check),
        Err("m1 is already a machine".into())
    );
    assert_eq!(w.model.add("vps", "vps.test", "agent", &check), Ok(()));
    let saved = w.registry.load().machines();
    let vps = saved.iter().find(|m| m.name == "vps").unwrap();
    assert_eq!(vps.admin(), AdminAccess::RootSsh);
    assert_eq!(vps.os, Some(MachineOs::Linux));
    assert_eq!(
        vps.abilities
            .as_ref()
            .and_then(|a| a.home.clone())
            .as_deref(),
        Some("/home/agent")
    );
    assert_eq!(w.model.names(), ["m1", "vps"]);
    assert!(w.runner.calls().is_empty(), "add runs nothing");
}

#[test]
fn the_draft_check_refuses_an_unsafe_form_before_any_ssh() {
    let w = scripted_world(vec![]);
    let check = MachinesModel::draft_check(w.model.engine(), "Bad Name", "h", "u", &[]);
    assert!(!check.reachable && !check.problems.is_empty());
    assert!(
        w.runner.calls().is_empty(),
        "nothing ran for an unsafe form"
    );
    let check = MachinesModel::draft_check(w.model.engine(), "m2", "m2.test", "worker", &[]);
    assert_eq!(check.lines[0].title, "Reachable");
    assert_eq!(w.runner.calls().len(), 1, "one ssh for the draft's probes");
    assert!(w.runner.calls()[0].argv[0].ends_with("ssh"));
}

#[test]
fn set_up_shows_the_steps_exact_commands_and_runs_nothing_until_confirmed() {
    let mut w = scripted_world(vec![mac("m1")]);
    w.model.request_set_up(&StepId::BASE_TOOLS, "m1");
    let prompt = fix_sheet(&w.model).clone();
    assert_eq!(prompt.title(), "Base tools");
    assert_eq!(prompt.plan.kind, PlanKind::Fix(StepId::BASE_TOOLS));
    let shown = w.model.engine().steps(&mac("m1"));
    let display = shown
        .iter()
        .find(|s| s.id == StepId::BASE_TOOLS)
        .and_then(|s| s.plan().map(|p| p.display.clone()))
        .unwrap();
    assert_eq!(
        prompt.text(),
        display,
        "the sheet shows the Fix's own commands"
    );
    assert_eq!(prompt.intro(), "These commands run on worker@m1.test:");
    assert!(w.runner.calls().is_empty(), "showing a plan runs nothing");

    // Cancel: the plan is forgotten, so a confirm of it would fail; nothing ran
    w.model.cancel_sheet();
    assert!(w.model.sheet.is_none());
    assert!(w.model.confirm().is_none());
    assert!(w.runner.calls().is_empty());

    // Run: without Reachable ✓ the engine refuses, and the card gets the line
    w.model.request_set_up(&StepId::BASE_TOOLS, "m1");
    let run = w.model.confirm().expect("a confirmed run");
    assert_eq!(run.machine, "m1");
    let (done, outcome) = run.execute(w.model.engine());
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("waits for Reachable"),
        "{}",
        outcome.message
    );
    w.model.finished(&done, &outcome);
    let line = &w.model.outcomes["m1"];
    assert!(!line.ok && !line.guidance);
}

#[test]
fn set_up_all_with_nothing_checked_is_a_note_not_a_failure() {
    let mut w = off_world(vec![mac("m1")]);
    w.model.request_set_up_all("m1");
    assert!(w.model.sheet.is_none());
    let line = &w.model.outcomes["m1"];
    assert!(line.ok && line.text == "nothing to set up");
}

#[test]
fn the_running_switch_waits_for_the_steps_before_it() {
    let mut w = off_world(vec![with_job(mac("m1"))]);
    let m = w.model.machine("m1").unwrap();
    let sw = w.model.running_switch(&m);
    assert!(!sw.on && !sw.enabled, "no result yet");
    w.model.request_toggle("m1", true);
    assert!(w.model.sheet.is_none());
    assert!(w.model.outcomes["m1"].text.starts_with("waits for "));
    // off never waits: its plan shows (a launchd switch as the worker: no admin on a Mac)
    w.model.request_toggle("m1", false);
    let prompt = fix_sheet(&w.model);
    assert_eq!(prompt.plan.kind, PlanKind::Running(false));
    assert_eq!(prompt.plan.admin, AdminNeed::None);
    assert!(prompt.can_run(false));
    let run = w.model.confirm().expect("a run");
    assert_eq!(run.kind, PlanKind::Running(false));
}

#[test]
fn an_admin_step_needs_the_password_for_the_one_run() {
    let mut w = off_world(vec![mac("m1")]);
    w.model.request_set_up(&StepId::STAYS_AWAKE, "m1");
    let prompt = fix_sheet(&w.model);
    assert_eq!(prompt.plan.admin, AdminNeed::Password);
    assert!(prompt.needs_password() && !prompt.can_run(false) && prompt.can_run(true));
    assert!(prompt.text().starts_with("as admin:"));
    // confirm without a password is refused and recorded
    assert!(w.model.confirm().is_none());
    assert_eq!(w.model.outcomes["m1"].text, "needs the admin password");
    // with one, the run is handed back; the password is in the answer and nowhere in the model
    w.model.request_set_up(&StepId::STAYS_AWAKE, "m1");
    w.model.password_field().unwrap().push_str("hunter2");
    let run = w.model.confirm().expect("a run");
    assert_eq!(run.kind, PlanKind::Fix(StepId::STAYS_AWAKE));
    assert!(w.model.sheet.is_none());
    assert!(!format!("{:?}", w.model.outcomes).contains("hunter2"));
}

#[test]
fn an_off_tool_says_so_in_every_check() {
    let w = off_world(vec![mac("m1")]);
    w.model.engine().refresh(None, true);
    let results = w.model.engine().results("m1");
    let reachable = &results[&StepId::REACHABLE];
    assert!(reachable.state.is_fail());
    assert!(
        reachable.state.text().contains("Machines is off"),
        "{}",
        reachable.state.text()
    );
    assert!(results[&StepId::BASE_TOOLS]
        .state
        .text()
        .contains("not reachable"));
}

#[test]
fn secret_rows_say_what_the_vault_has_and_send_again_shows_the_delivery() {
    let mut w = off_world(vec![with_job(mac("m1"))]);
    let m = w.model.machine("m1").unwrap();
    let rows = w.model.secret_rows(&m);
    assert_eq!(rows.len(), 2);
    assert!(rows[0]
        .text
        .starts_with("claude-worker → ~/.config/agents/claude.env"));
    assert!(rows[0].text.ends_with("not in the vault") && !rows[0].can_send);
    assert_eq!(rows[1].usage.name, "m1-github-issues");

    let mut entry = VaultEntry::new("claude-worker", Kind::Token);
    entry.expires = Some("2027-01-15".into());
    lock(&w.vault.entries).push(entry);
    w.model.reload();
    let rows = w.model.secret_rows(&m);
    assert!(rows[0].can_send && rows[0].text.contains("expires"));
    w.model.request_send_again(&rows[0].usage);
    let prompt = fix_sheet(&w.model);
    assert_eq!(prompt.plan.kind, PlanKind::SendAgain(rows[0].usage.clone()));
    assert!(prompt.text().contains("claude.env"), "{}", prompt.text());
    assert!(!prompt.text().contains("hunter"), "no value in a display");
}

#[test]
fn a_renewed_vault_value_offers_an_update_whose_plan_names_no_value() {
    let mut w = off_world(vec![with_job(mac("m1")), with_job(mac("m2"))]);
    assert!(w.model.update_offers().is_empty());
    w.model.engine().vault_value_changed("claude-worker");
    let offers = w.model.update_offers();
    assert_eq!(offers.len(), 1);
    assert_eq!(
        (offers[0].secret.as_str(), offers[0].count),
        ("claude-worker", 2)
    );
    assert_eq!(offers[0].title(), "Update 2 machines");
    w.model.request_update("claude-worker");
    let prompt = fix_sheet(&w.model);
    assert_eq!(prompt.plan.kind, PlanKind::Update("claude-worker".into()));
    assert_eq!(prompt.plan.steps.len(), 2);
    assert!(prompt.intro().contains("one machine at a time"));
    w.model.cancel_sheet();
    w.model.request_update("nobody-waits");
    assert!(w.model.update_notes["nobody-waits"].contains("no machine waits"));
}

#[test]
fn remove_and_admin_change_the_record_only() {
    let mut w = scripted_world(vec![mac("m1"), mac("m2")]);
    w.model.set_admin("m1", AdminAccess::RootSsh);
    assert_eq!(w.model.machine("m1").unwrap().admin(), AdminAccess::RootSsh);
    w.model
        .outcomes
        .insert("m2".into(), super::pieces::FixLine::note("x"));
    w.model.remove("m2");
    assert_eq!(w.model.names(), ["m1"]);
    assert!(!w.model.outcomes.contains_key("m2"));
    assert!(
        w.runner.calls().is_empty(),
        "the machine itself is never touched"
    );
    assert_eq!(*lock(&w.registry.saves), 2);
}

#[test]
fn save_job_checks_the_draft_then_keeps_what_the_sheet_never_edits() {
    let mut w = off_world(vec![with_job(mac("m1"))]);
    let mut draft = JobDraft::of(&w.model.machine("m1").unwrap());
    draft.repos[0].github = "bad name".into();
    assert_eq!(
        w.model.save_job(&draft).unwrap_err(),
        "Repository: owner/name, like me/app"
    );
    draft.repos[0].github = "me/other".into();
    draft.kinds = vec!["backend".into()];
    assert_eq!(w.model.save_job(&draft), Ok(()));
    let job = w.model.machine("m1").unwrap().sd_worker().cloned().unwrap();
    assert_eq!(job.repos[0].github, "me/other");
    assert_eq!(job.kinds, ["backend"]);
    assert_eq!(job.sd_path, "/Users/worker/sd/sd.py", "kept from the base");
    assert_eq!(job.issues_secret, "m1-github-issues");
}

#[test]
fn set_up_all_asks_for_an_admin_step_through_the_sheet() {
    let mut w = off_world(vec![mac("m1")]);
    let asks = w.model.asks.clone();
    let step = ApprovedStep {
        id: StepId::STAYS_AWAKE,
        title: "Stays awake".into(),
        display: "sudo pmset -a sleep 0".into(),
        waiting: false,
        needs_admin: true,
    };
    let asked = std::thread::spawn(move || asks.ask("m1", &step));
    // the tick picks the ask up once it is there
    let mut shown = false;
    for _ in 0..200 {
        if w.model.tick() {
            shown = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(shown, "the admin ask reached the sheet");
    let prompt = fix_sheet(&w.model);
    assert!(prompt.asked && prompt.needs_password());
    assert_eq!(prompt.text(), "sudo pmset -a sleep 0");
    assert!(
        {
            w.model.password_field().unwrap().push_str("pw");
            w.model.confirm().is_none()
        },
        "the answer goes to the run, not a new plan"
    );
    let answer = asked.join().unwrap();
    assert!(matches!(answer, AdminAnswer::Password(p) if p.expose() == "pw"));
    assert!(w.model.sheet.is_none());

    // a cancel answers None
    let asks = w.model.asks.clone();
    let step = ApprovedStep {
        id: StepId::STAYS_AWAKE,
        title: "Stays awake".into(),
        display: "x".into(),
        waiting: false,
        needs_admin: true,
    };
    let asked = std::thread::spawn(move || asks.ask("m1", &step));
    for _ in 0..200 {
        if w.model.tick() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    w.model.cancel_sheet();
    assert!(matches!(asked.join().unwrap(), AdminAnswer::None));
}

#[test]
fn the_morning_items_follow_the_poller_and_a_done_line_leaves_with_its_item() {
    let mut w = off_world(vec![with_job(mac("m1"))]);
    assert!(w.model.items.is_empty());
    let fetch = w.model.reload().expect("repos to fetch");
    assert_eq!(fetch.repos, ["me/app"]);
    let result = fetch.run(&*w.model.parts.runner);
    assert!(result.1.contains("me/app"), "gh is refused while off");
    w.model.issues_fetched(fetch.generation, result);
    assert!(w.model.github_note.as_deref().unwrap().contains("me/app"));
    w.model
        .issues_fetched(fetch.generation + 7, (BTreeMap::new(), Default::default()));
    w.model.done.insert("gone".into(), "Merged".into());
    w.model.acted("gone", Some("Merged"));
    assert!(w.model.done.is_empty(), "no item with that id any more");
}
