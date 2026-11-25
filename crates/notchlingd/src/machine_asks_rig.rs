use super::machine_asks::MachineAsks;
use super::machines_ops::Machines;
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_asks::Admin;
use crate::consent_ops::ConsentOps;
use crate::consent_testkit::{agent_caller, app_caller};
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::limits_ops_tests::TempDir;
use notchling_engine::accounts::Secret;
use notchling_engine::consent::ConsentStore;
use notchling_engine::machines::addresses::{AddressKind, MachineAddress};
use notchling_engine::machines::admin_secret::AdminSecrets;
use notchling_engine::machines::engine::{Catalog, Deps, SetupEngine};
use notchling_engine::machines::github::NoGitHub;
use notchling_engine::machines::registry::{Machine, RegistryFiles};
use notchling_engine::machines::roots::MachineRoots;
use notchling_engine::machines::runner::{CommandResult, ScriptedRunner};
use notchling_engine::machines::step::{
    Cadence, CheckResult, FixOutcome, FixPlan, MacFacts, NoOpener, NoSecrets, Probe, SetupStep,
    StepFix, StepId,
};
use notchling_engine::machines::steps_tailscale_linux::SIGN_IN;
use notchling_engine::machines::tailscale::Tailscale;
use notchling_engine::organizations::{Organizations, PERSONAL_ID};
use notchling_events::{MemorySink, Salt};
use notchling_local_api::{ops, AgentIdentity, ApiError, Caller};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub const LIST: &str = "ID    Tailnet              Account\na1b2  person@example.com   person@example.com*\nc3d4  work.example         work@example.com\n";
pub const STATUS: &str = r#"{"BackendState":"Running","Self":{"HostName":"mac","DNSName":"mac.tail0a1b2.ts.net.","OS":"macOS","UserID":1001,"TailscaleIPs":["100.64.0.10"],"Online":true},"CurrentTailnet":{"Name":"person@example.com","MagicDNSSuffix":"tail0a1b2.ts.net"},"Peer":{"nodekey:aa":{"ID":"n1","HostName":"fresh","DNSName":"fresh.tail0a1b2.ts.net.","OS":"linux","UserID":1002,"TailscaleIPs":["100.64.0.11"],"Online":true,"Tags":["tag:secret-role"]}},"User":{"1001":{"ID":1001,"LoginName":"person@example.com","DisplayName":"Person"},"1002":{"ID":1002,"LoginName":"friend@example.com","DisplayName":"Friend"}}}"#;
pub const PASSWORD: &str = "hunter2-admin-pass";

pub struct FakeMembers {
    pub node: Mutex<Option<(String, String)>>,
}

impl Admin for FakeMembers {
    fn status(&self) -> Value {
        json!({"cluster": {"id": "c1"}, "members": [
            {"id": "aaaa", "state": "active"}, {"id": "bbbb", "state": "active"}],
            "failover": {"value": "off"}})
    }
    fn own_organization(&self) -> Option<String> {
        Some(PERSONAL_ID.to_string())
    }
    fn organization_of(&self, _machine: &str) -> Option<String> {
        None
    }
    fn host_of(&self, _machine: &str) -> Option<String> {
        None
    }
    fn matches(&self, _machine: &str) -> usize {
        1
    }
    fn node_of(&self, machine: &str) -> Option<String> {
        self.node
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(m, _)| m == machine)
            .map(|(_, n)| n.clone())
    }
    fn secret_organization(&self, _name: &str) -> Option<String> {
        None
    }
}

pub struct FixedAdmin;

impl AdminSecrets for FixedAdmin {
    fn read(&self, entry: &str, _machine: &str) -> Result<Secret, String> {
        if entry.is_empty() || entry != entry.trim() {
            return Err(format!("{entry} isn't in the Vault."));
        }
        Ok(Secret::new(PASSWORD))
    }
}

pub struct Rig {
    pub(crate) tmp: TempDir,
    pub sink: Arc<MemorySink>,
    pub consent: Arc<ConsentOps>,
    pub machines: Arc<Machines>,
    pub asks: Arc<MachineAsks>,
    pub on: Arc<AtomicBool>,
    pub who: Arc<Mutex<AgentIdentity>>,
    pub ssh: Arc<ScriptedRunner>,
    pub tail: Arc<ScriptedRunner>,
    pub members: Arc<FakeMembers>,
    pub shown: Arc<Mutex<String>>,
    pub fixed: Arc<Mutex<Vec<String>>>,
    pub passwords: Arc<Mutex<Vec<String>>>,
    pub changes: Arc<Mutex<usize>>,
    pub sign_in: Arc<AtomicBool>,
    pub clock: Arc<Mutex<f64>>,
    pub slow_runs: Arc<AtomicUsize>,
    pub lists: Arc<AtomicUsize>,
    pub drift: Arc<Mutex<Option<(usize, String)>>>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.consent.close();
    }
}

pub fn machine(name: &str, org: Option<&str>) -> Machine {
    let mut m = Machine::new(name, &format!("{name}.test"), "worker");
    m.organization = org.map(str::to_string);
    m
}

pub fn with_entry(mut m: Machine, entry: &str) -> Machine {
    m.set_admin_secret(Some(entry));
    m
}

pub fn on_tailscale(mut m: Machine) -> Machine {
    let host = format!("{}.tail0a1b2.ts.net", m.name);
    m.set_addresses(vec![MachineAddress::new(AddressKind::TailscaleName, &host)]);
    m
}

fn fix_step(
    id: &'static str,
    admin: bool,
    display: String,
    fixed: Arc<Mutex<Vec<String>>>,
    passwords: Arc<Mutex<Vec<String>>>,
) -> SetupStep {
    let done = fixed.clone();
    SetupStep::new(
        StepId(id.into()),
        "A step",
        vec![],
        Cadence::EveryRefresh,
        Probe::Local(Arc::new(move |_| {
            if done.lock().unwrap().iter().any(|x| x == id) {
                CheckResult::pass("done")
            } else {
                CheckResult::fail("not yet")
            }
        })),
        StepFix::Run(FixPlan::new(display, admin, move |ctx| {
            if let Some(p) = &ctx.admin_password {
                passwords.lock().unwrap().push(p.expose().to_string());
            }
            fixed.lock().unwrap().push(id.to_string());
            FixOutcome::ok("done")
        })),
    )
}

pub fn rig(tag: &str, list: Vec<Machine>) -> Rig {
    rig_with(tag, list, false)
}

pub fn rig_standard(tag: &str, list: Vec<Machine>) -> Rig {
    rig_with(tag, list, true)
}

fn rig_with(tag: &str, list: Vec<Machine>, standard: bool) -> Rig {
    let tmp = TempDir::new(&format!("machine-asks-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let sink = Arc::new(MemorySink::new());
    let consent = ConsentOps::with_clock(
        Arc::new(ConsentStore::in_memory_for_tests()),
        room,
        Some(inbox),
        Some(Events::fixed(sink.clone(), Salt([3; 32]))),
        Arc::new(crate::acting_ops::now_ms),
    );
    let mut me = crate::consent_testkit::verified("parent");
    me.organization = Some(PERSONAL_ID.to_string());
    let who = Arc::new(Mutex::new(me));
    let seen = who.clone();
    consent.bind_identity(Arc::new(move |_| seen.lock().unwrap().clone()));
    consent.start();

    let roots = MachineRoots::in_data(tmp.root());
    let registry: Arc<RegistryFiles> = Arc::new(roots.registry());
    assert!(registry.save(&list));
    let ssh = Arc::new(ScriptedRunner::new());
    let tail = Arc::new(ScriptedRunner::new());
    tail.on(
        |a| a.get(2).map(String::as_str) == Some("--list"),
        CommandResult::ok(LIST),
    );
    tail.on(
        |a| a.get(1).map(String::as_str) == Some("status"),
        CommandResult::ok(STATUS),
    );
    let shown = Arc::new(Mutex::new("echo fix".to_string()));
    let fixed = Arc::new(Mutex::new(Vec::new()));
    let passwords = Arc::new(Mutex::new(Vec::new()));
    let sign_in = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(Mutex::new(1_800_000_000.0));
    let slow_runs = Arc::new(AtomicUsize::new(0));
    let lists = Arc::new(AtomicUsize::new(0));
    let drift: Arc<Mutex<Option<(usize, String)>>> = Arc::new(Mutex::new(None));
    let now = clock.clone();
    let deps = Deps {
        runner: ssh.clone(),
        secrets: Arc::new(NoSecrets),
        http: Arc::new(NoGitHub),
        opener: Arc::new(NoOpener),
        store: registry.clone(),
        mac: Arc::new(MacFacts::default),
        clock: Arc::new(move || *now.lock().unwrap()),
        scratch: tmp.path("scratch"),
        offers_data: Some(tmp.root().to_path_buf()),
        audit: Some(roots.audit_file()),
    };
    let catalog = if standard {
        notchling_engine::machines::catalog::standard()
    } else {
        let (s, f, p, g) = (
            shown.clone(),
            fixed.clone(),
            passwords.clone(),
            sign_in.clone(),
        );
        let (calls, late, slow) = (lists.clone(), drift.clone(), slow_runs.clone());
        Catalog {
            steps: Arc::new(move |_, _| {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                let mut shown = s.lock().unwrap().clone();
                if let Some((at, text)) = late.lock().unwrap().clone() {
                    if n >= at {
                        shown = text;
                    }
                }
                let counted = slow.clone();
                let mut list = vec![
                    fix_step("plain", false, shown.clone(), f.clone(), p.clone()),
                    fix_step("rooted", true, shown, f.clone(), p.clone()),
                    SetupStep::new(
                        StepId("slowone".into()),
                        "Slow",
                        vec![],
                        Cadence::Slow,
                        Probe::Local(Arc::new(move |_| {
                            counted.fetch_add(1, Ordering::SeqCst);
                            CheckResult::pass("ok")
                        })),
                        StepFix::HowTo("n/a".into()),
                    ),
                ];
                if g.load(Ordering::SeqCst) {
                    list.push(fix_step(
                        "node-tailscale",
                        true,
                        SIGN_IN.to_string(),
                        f.clone(),
                        p.clone(),
                    ));
                }
                list
            }),
            usages: Arc::new(|_| Vec::new()),
            deliver: Arc::new(|_, _| FixOutcome::ok("")),
            toggle_script: Arc::new(|_, _| "toggle".to_string()),
            offers_replace: Arc::new(|_| false),
        }
    };
    let engine = Arc::new(SetupEngine::new(deps, catalog).with_admin_secrets(Arc::new(FixedAdmin)));
    let members = Arc::new(FakeMembers {
        node: Mutex::new(None),
    });
    let on = Arc::new(AtomicBool::new(true));
    let flag = on.clone();
    let changes = Arc::new(Mutex::new(0usize));
    let counted = changes.clone();
    let machines = Arc::new(Machines {
        engine,
        registry,
        orgs: Organizations::new(tmp.root()),
        tailscale: Tailscale::new(tail.clone(), "/fake/tailscale"),
        members: members.clone(),
        on: Arc::new(move || flag.load(Ordering::SeqCst)),
        consent: consent.clone(),
        changed: Arc::new(move || *counted.lock().unwrap() += 1),
    });
    let asks = MachineAsks::new(machines.clone());
    Rig {
        tmp,
        sink,
        consent,
        machines,
        asks,
        on,
        who,
        ssh,
        tail,
        members,
        shown,
        fixed,
        passwords,
        changes,
        sign_in,
        clock,
        slow_runs,
        lists,
        drift,
    }
}

pub fn reason(e: &ApiError) -> String {
    e.data
        .as_ref()
        .and_then(|d| d["reason"].as_str())
        .unwrap_or_default()
        .to_string()
}

impl Rig {
    pub fn read(&self, op: &str, args: Value) -> Result<Value, ApiError> {
        use crate::op_group::OpGroup;
        super::machines_ops::Reads(self.machines.clone()).call(&agent_caller(), op, &args)
    }

    pub fn ask(&self, action: &str, args: Value) -> Result<Value, ApiError> {
        use crate::op_group::OpGroup;
        self.asks.call(
            &agent_caller(),
            ops::MACHINES_REQUEST,
            &json!({"action": action, "args": args}),
        )
    }

    pub fn card(&self, action: &str, args: Value) -> String {
        let reply = self.ask(action, args).unwrap();
        assert_eq!(reply["state"], "waiting", "{reply}");
        reply["request"]["id"].as_str().unwrap().to_string()
    }

    pub fn apply_with(&self, id: &str, admin: Option<Value>) -> Result<Value, ApiError> {
        use crate::cited::Cited;
        let row = self.consent.get_row(id).unwrap();
        let op = row.change["apply"]["op"].as_str().unwrap().to_string();
        let mut args = row.change["apply"]["args"].clone();
        args["consentId"] = json!(id);
        if let Some(admin) = admin {
            args["admin"] = admin;
        }
        let name = op.clone();
        let tail = self.tail.clone();
        let tailscale = Tailscale::new(tail, "/fake/tailscale");
        self.asks.apply(&app_caller(), &name, &args, &move |a| {
            let tailnet = a["tailnet"].as_str().unwrap_or_default().to_string();
            tailscale
                .switch(&tailnet)
                .map(|()| json!({"switched": true}))
                .map_err(|e| ApiError::new(notchling_local_api::ErrorCode::Failed, e))
        })
    }

    pub fn apply(&self, id: &str) -> Result<Value, ApiError> {
        self.apply_with(id, None)
    }

    pub fn registry_names(&self) -> Vec<String> {
        self.machines.all().iter().map(|m| m.name.clone()).collect()
    }

    pub fn password_secret() -> Secret {
        Secret::new(PASSWORD)
    }

    pub fn ssh_text(&self) -> String {
        self.ssh
            .calls()
            .iter()
            .map(|c| c.argv.join(" "))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn tail_calls(&self) -> Vec<Vec<String>> {
        self.tail.calls().into_iter().map(|c| c.argv).collect()
    }

    pub fn rows(&self) -> Vec<Value> {
        use crate::op_group::OpGroup;
        let reply = self
            .consent
            .call(&app_caller(), ops::CONSENT_LIST, &json!({}))
            .unwrap();
        reply["requests"].as_array().cloned().unwrap_or_default()
    }

    pub fn as_caller(&self) -> Caller {
        agent_caller()
    }
}
