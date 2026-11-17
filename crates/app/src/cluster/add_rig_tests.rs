use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use notchling_engine::machines::catalog::standard;
use notchling_engine::machines::engine::{Catalog, Deps, MachineStore, SetupEngine};
use notchling_engine::machines::github::NoGitHub;
use notchling_engine::machines::registry::{
    Machine, MachineAbilities, MachineOs, RegistryState, SdWorkerSettings,
};
use notchling_engine::machines::runner::{Call, ScriptedRunner};
use notchling_engine::machines::step::{
    Cadence, CheckResult, Detected, FixOutcome, FixPlan, MacFacts, NoCluster, NoOpener, Probe,
    SetupStep, StepFix, StepId,
};
use serde_json::{json, Value};

use super::add::{AddFlow, End};
use super::add_parts::{Account, AddParts};
use crate::machines::model::Registry;

pub const NODE: &str = "0123456789abcdef";
pub const T0: f64 = 1_800_000_000.0;

pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
pub struct MemRegistry {
    pub list: Mutex<Vec<Machine>>,
    pub saves: Mutex<usize>,
}

impl Registry for MemRegistry {
    fn load(&self) -> RegistryState {
        RegistryState::Loaded {
            machines: lock(&self.list).clone(),
            migrated: false,
        }
    }
    fn save(&self, machines: &[Machine]) -> bool {
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

pub struct World {
    pub unreachable: Mutex<Option<String>>,
    pub os: Mutex<MachineOs>,
    pub version: Mutex<String>,
    pub arch: Mutex<String>,
    pub file_vault: Mutex<Option<bool>>,
    pub tailscale: Mutex<bool>,
    pub reach: Mutex<bool>,
    pub rival: Mutex<bool>,
    pub done: Mutex<BTreeSet<String>>,
    pub fails: Mutex<BTreeSet<String>>,
    pub fixes: Mutex<Vec<String>>,
    pub app: Mutex<Option<bool>>,
    pub app_open: Mutex<bool>,
    pub git: Mutex<bool>,
    pub codex: Mutex<bool>,
    pub unsure: Mutex<BTreeSet<String>>,
}

impl World {
    pub fn new() -> Arc<World> {
        Arc::new(World {
            unreachable: Mutex::new(None),
            os: Mutex::new(MachineOs::MacOs),
            version: Mutex::new("14.5".into()),
            arch: Mutex::new("arm64".into()),
            file_vault: Mutex::new(Some(true)),
            tailscale: Mutex::new(true),
            reach: Mutex::new(true),
            rival: Mutex::new(false),
            done: Mutex::new(BTreeSet::new()),
            fails: Mutex::new(BTreeSet::new()),
            fixes: Mutex::new(Vec::new()),
            app: Mutex::new(None),
            app_open: Mutex::new(false),
            git: Mutex::new(true),
            codex: Mutex::new(true),
            unsure: Mutex::new(BTreeSet::new()),
        })
    }

    pub fn fixed(&self) -> Vec<String> {
        lock(&self.fixes).clone()
    }

    pub fn set_done(&self, names: &[&str]) {
        *lock(&self.done) = names.iter().map(|n| n.to_string()).collect();
    }
}

fn passing(world: &Arc<World>, name: &'static str, text: &'static str) -> Probe {
    let w = world.clone();
    Probe::local(move |_| {
        if lock(&w.done).contains(name) {
            if lock(&w.unsure).contains(name) {
                return CheckResult::cant_tell(format!("can't tell about {name}"));
            }
            CheckResult::pass(text)
        } else {
            CheckResult::fail(format!("{name} isn't there"))
        }
    })
}

fn fix(world: &Arc<World>, name: &'static str, admin: bool, shows: String) -> StepFix {
    let w = world.clone();
    StepFix::Run(FixPlan::new(shows, admin, move |ctx| {
        let _ = ctx
            .step
            .runner
            .run(&Call::secs(vec![format!("fix-{name}")], 1));
        lock(&w.fixes).push(name.to_owned());
        if lock(&w.fails).contains(name) {
            let said = ctx
                .admin_password
                .as_ref()
                .map(|p| format!(" (sudo said {})", p.expose()))
                .unwrap_or_default();
            return FixOutcome::fail(format!("could not {name}{said}"));
        }
        lock(&w.done).insert(name.to_owned());
        FixOutcome::ok(format!("did {name}"))
    }))
}

fn step(id: StepId, title: &str, needs: &[StepId], probe: Probe, fix: StepFix) -> SetupStep {
    SetupStep::new(id, title, needs.to_vec(), Cadence::EveryRefresh, probe, fix)
}

fn catalog(world: &Arc<World>) -> Catalog {
    let w = world.clone();
    let steps = move |m: &Machine, _all: &[Machine]| -> Vec<SetupStep> {
        let os = m.os.unwrap_or(*lock(&w.os));
        let strict_home = m
            .abilities
            .as_ref()
            .and_then(|a| a.home.clone())
            .unwrap_or_else(|| "an unknown home".to_owned());
        let home = m
            .abilities
            .as_ref()
            .and_then(|a| a.home.clone())
            .unwrap_or_else(|| format!("/Users/{}", m.user));
        let reach_probe = {
            let w = w.clone();
            let home = home.clone();
            Probe::local(move |_| {
                if let Some(why) = lock(&w.unreachable).clone() {
                    return CheckResult::fail(why);
                }
                let abilities = MachineAbilities {
                    arch: Some(lock(&w.arch).clone()),
                    os_version: Some(lock(&w.version).clone()),
                    home: Some(home.clone()),
                    ..MachineAbilities::default()
                };
                CheckResult::pass("up").detected(Detected {
                    os: Some(*lock(&w.os)),
                    home: Some(home.clone()),
                    abilities: Some(abilities),
                    notchling_app: *lock(&w.app),
                    ..Detected::default()
                })
            })
        };
        let mut out = vec![step(
            StepId::REACHABLE,
            "Reachable",
            &[],
            reach_probe,
            StepFix::HowTo("fix ssh".into()),
        )];
        if m.node.is_some() {
            let node_only = m.sd_worker().is_none();
            if os == MachineOs::MacOs {
                let w2 = w.clone();
                out.push(step(
                    StepId::FILEVAULT,
                    "FileVault",
                    &[StepId::REACHABLE],
                    Probe::local(move |_| {
                        let fv = *lock(&w2.file_vault);
                        CheckResult::pass("read").detected(Detected {
                            file_vault: fv,
                            ..Detected::default()
                        })
                    }),
                    StepFix::HowTo("turn it on".into()),
                ));
            }
            if node_only {
                let w2 = w.clone();
                out.push(step(
                    StepId::NODE_GIT,
                    "Git",
                    &[StepId::REACHABLE],
                    Probe::local(move |_| {
                        if *lock(&w2.git) {
                            CheckResult::pass("git 2")
                        } else {
                            CheckResult::fail("git isn't installed")
                        }
                    }),
                    StepFix::HowTo("install git".into()),
                ));
            }
            let w2 = w.clone();
            out.push(step(
                StepId::NODE_TAILSCALE,
                "Tailscale connected",
                &[StepId::REACHABLE],
                Probe::local(move |_| {
                    if *lock(&w2.tailscale) {
                        CheckResult::pass("running")
                    } else {
                        CheckResult::fail("Tailscale isn't running there")
                    }
                }),
                StepFix::HowTo("install Tailscale".into()),
            ));
            out.push(step(
                StepId::NODE_BINARY,
                "Notchling installed",
                &[StepId::REACHABLE],
                passing(&w, "binary", "built"),
                fix(&w, "binary", false, format!("copy to {home}")),
            ));
            out.push(step(
                StepId::NODE_SERVICE,
                "Notchling service",
                &[StepId::NODE_BINARY],
                passing(&w, "service", "loaded"),
                fix(
                    &w,
                    "service",
                    true,
                    format!("write the plist in {strict_home}"),
                ),
            ));
            if m.runs_app() {
                let w2 = w.clone();
                out.push(step(
                    StepId::NODE_APP_CLOSED,
                    "Notchling app closed",
                    &[StepId::NODE_BINARY],
                    Probe::local(move |_| {
                        if *lock(&w2.app_open) {
                            CheckResult::fail("the app is open there")
                        } else {
                            CheckResult::pass("closed")
                        }
                    }),
                    StepFix::HowTo("quit the app".into()),
                ));
            }
            let w2 = w.clone();
            out.push(step(
                StepId::NODE_REACH,
                "Reaches the Primary",
                &[StepId::NODE_SERVICE, StepId::NODE_TAILSCALE],
                Probe::local(move |_| {
                    if *lock(&w2.reach) {
                        CheckResult::pass("reached")
                    } else {
                        CheckResult::fail("port 47823 is blocked")
                    }
                }),
                StepFix::HowTo("open the ACL".into()),
            ));
            let w2 = w.clone();
            let mut joined_needs = vec![
                StepId::NODE_SERVICE,
                StepId::NODE_TAILSCALE,
                StepId::NODE_REACH,
            ];
            if m.runs_app() {
                joined_needs.push(StepId::NODE_APP_CLOSED);
            }
            out.push(step(
                StepId::NODE_JOINED,
                "Joined the cluster",
                &joined_needs,
                Probe::local(move |_| {
                    let seen = |r: CheckResult| {
                        r.detected(Detected {
                            node: Some(NODE.into()),
                            ..Detected::default()
                        })
                    };
                    if *lock(&w2.rival) {
                        return seen(CheckResult::fail("in another cluster, rival"));
                    }
                    if lock(&w2.done).contains("joined") {
                        seen(CheckResult::pass("in home"))
                    } else {
                        seen(CheckResult::fail("not in a cluster yet"))
                    }
                }),
                fix(&w, "joined", false, "ask to join".into()),
            ));
            let w2 = w.clone();
            out.push(step(
                StepId::NODE_CODEX_LOGIN,
                "Codex signed in",
                &[StepId::REACHABLE],
                Probe::local(move |_| {
                    if *lock(&w2.codex) {
                        CheckResult::pass("signed in")
                    } else {
                        CheckResult::fail("not signed in")
                    }
                }),
                StepFix::HowTo("sign Codex in".into()),
            ));
        }
        if m.sd_worker().is_some() {
            out.push(step(
                StepId::SERVICE,
                "sd service",
                &[StepId::REACHABLE],
                passing(&w, "sd-service", "loaded"),
                fix(&w, "sd-service", false, "start the sd service".into()),
            ));
        }
        out
    };
    let real = standard();
    Catalog {
        steps: Arc::new(steps),
        usages: real.usages,
        deliver: real.deliver,
        toggle_script: real.toggle_script,
        offers_replace: real.offers_replace,
    }
}

pub struct Rig {
    pub world: Arc<World>,
    pub registry: Arc<MemRegistry>,
    pub runner: Arc<ScriptedRunner>,
    pub engine: Arc<SetupEngine>,
}

pub fn this_mac() -> Account {
    Account {
        user: "sondoan".into(),
        home: "/Users/sondoan".into(),
    }
}

impl Rig {
    pub fn new(machines: Vec<Machine>) -> Rig {
        let world = World::new();
        let registry = Arc::new(MemRegistry::default());
        *lock(&registry.list) = machines;
        let runner = Arc::new(ScriptedRunner::new());
        let store: Arc<dyn MachineStore> = registry.clone();
        let deps = Deps {
            runner: runner.clone(),
            secrets: Arc::new(notchling_engine::machines::step::NoSecrets),
            http: Arc::new(NoGitHub),
            opener: Arc::new(NoOpener),
            store,
            mac: Arc::new(MacFacts::default),
            clock: Arc::new(|| T0),
            scratch: std::env::temp_dir().join("notchling-add-tests"),
            offers_data: None,
            audit: None,
        };
        let engine = Arc::new(
            SetupEngine::new(deps, catalog(&world)).with_cluster(Arc::new(NoCluster), None),
        );
        Rig {
            world,
            registry,
            runner,
            engine,
        }
    }

    pub fn real(machines: Vec<Machine>) -> Rig {
        let rig = Rig::new(machines);
        let deps = Deps {
            runner: rig.runner.clone(),
            secrets: Arc::new(notchling_engine::machines::step::NoSecrets),
            http: Arc::new(NoGitHub),
            opener: Arc::new(NoOpener),
            store: rig.registry.clone(),
            mac: Arc::new(MacFacts::default),
            clock: Arc::new(|| T0),
            scratch: std::env::temp_dir().join("notchling-add-tests"),
            offers_data: None,
            audit: None,
        };
        let engine =
            Arc::new(SetupEngine::new(deps, standard()).with_cluster(Arc::new(NoCluster), None));
        Rig { engine, ..rig }
    }

    pub fn answer_batches(&self, answer: impl Fn(&str) -> (String, i32) + Send + Sync + 'static) {
        self.runner.on_call(
            |argv| {
                argv.last()
                    .is_some_and(|s| s.contains("out=$( /bin/sh -c "))
            },
            move |call| {
                let script = call.argv.last().cloned().unwrap_or_default();
                let at = script.find("@@").unwrap_or(0);
                let nonce: String = script[at + 2..].chars().take(16).collect();
                let mut out = String::new();
                for (i, seg) in script.split("out=$( /bin/sh -c ").skip(1).enumerate() {
                    let probe = seg.split(" </dev/null").next().unwrap_or("");
                    let (text, code) = answer(probe);
                    out.push_str(&format!("{text}\n@@{nonce} {i} {code}@@\n"));
                }
                notchling_engine::machines::runner::CommandResult::ok(&out)
            },
        );
    }

    pub fn parts(&self) -> AddParts {
        AddParts {
            engine: self.engine.clone(),
            registry: self.registry.clone(),
            this: this_mac(),
            off: None,
        }
    }

    pub fn flow(&self, typed: &str) -> AddFlow {
        let parts = self.parts();
        let existing = parts.names();
        AddFlow::from_address(parts, typed, &existing).expect("a good address")
    }

    pub fn check(&self, flow: &mut AddFlow) {
        let mut probes = flow.check();
        while let Some(probe) = probes.pop() {
            let reply = probe.run(&self.engine);
            probes.extend(flow.reply(reply));
        }
    }

    pub fn run_job(&self, flow: &mut AddFlow) -> End {
        let job = flow.take_job().expect("a job after run");
        let seen = std::cell::RefCell::new(Vec::new());
        let end = job.execute(&self.engine, &|p| seen.borrow_mut().push(p));
        for p in seen.into_inner() {
            flow.progress(p);
        }
        flow.finished(end.clone(), T0);
        end
    }

    pub fn saved(&self, name: &str) -> Option<Machine> {
        lock(&self.registry.list)
            .iter()
            .find(|m| m.name == name)
            .cloned()
    }

    pub fn fix_calls(&self) -> usize {
        self.runner
            .calls()
            .iter()
            .filter(|c| c.argv.first().is_some_and(|a| a.starts_with("fix-")))
            .count()
    }
}

pub fn sd_machine(name: &str, user: &str) -> Machine {
    let mut m = Machine::new(name, &format!("{name}.test"), user);
    m.os = Some(MachineOs::MacOs);
    let mut job = SdWorkerSettings::new(name);
    job.sd_path = "/Users/worker/sd/sd.py".into();
    m.set_sd_worker(Some(job));
    m
}

pub fn status(primary_user: Option<(&str, &str)>, members: &[(&str, &str, &str, bool)]) -> Value {
    let mut versions = json!({
        "notchling": "0.9.0", "proto": 1, "features": ["dispatch"], "schema": 1,
        "os": "macos", "arch": "aarch64",
    });
    if let Some((user, home)) = primary_user {
        versions["host"] = json!({"encrypted": true, "home": home, "user": user, "free_mb": 90000});
    }
    let mut list = vec![json!({
        "id": "a1b2c3d4e5f60718", "name": "m5", "role": "home", "canBeMaster": true,
        "online": true, "client": false, "headless": false, "reference": true,
        "state": "active", "versions": versions,
    })];
    for (id, name, state, online) in members {
        list.push(json!({
            "id": id, "name": name, "role": "worker", "canBeMaster": false,
            "online": online, "client": false, "headless": true, "reference": false,
            "state": state,
        }));
    }
    json!({
        "cluster": {"id": "c1", "name": "home", "term": 1, "home": "a1b2c3d4e5f60718"},
        "me": "a1b2c3d4e5f60718",
        "members": list,
        "link": true, "tailscale": true, "listening": [47823],
    })
}
