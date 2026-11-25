use super::machines_ops::Machines;
use crate::cluster_asks::Admin;
use crate::cluster_asks_live::LiveAdmin;
use crate::consent_ops::ConsentOps;
use crate::daemon_wiring::Wired;
use crate::layout::Layout;
use crate::op_group::OpGroup;
pub use crate::tailscale_ops::machine_runner;
use notchling_engine::accounts::Secret;
use notchling_engine::cluster::ready::{checks_in, Checks};
use notchling_engine::machines::admin_secret::AdminSecrets;
use notchling_engine::machines::catalog;
use notchling_engine::machines::engine::{Deps, SetupEngine};
use notchling_engine::machines::github::NoGitHub;
use notchling_engine::machines::mac_facts;
use notchling_engine::machines::reach::Router;
use notchling_engine::machines::registry::RegistryFiles;
use notchling_engine::machines::release_download::CurlDownloads;
use notchling_engine::machines::roots::MachineRoots;
use notchling_engine::machines::runner::Runner;
use notchling_engine::machines::step::{ClusterTokens, NoOpener, NoSecrets};
use notchling_engine::machines::steps_node::dist_dir;
use notchling_engine::machines::steps_node_reach::is_tailscale_ip;
use notchling_engine::machines::tailscale::{default_cli, Tailscale};
use notchling_engine::organizations::Organizations;
use notchling_local_api::{ops, topics, Caller, ClientKind, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TAILNET_TTL: Duration = Duration::from_secs(60);

pub struct VaultAdmin(pub Arc<dyn OpGroup>);

impl AdminSecrets for VaultAdmin {
    fn read(&self, entry: &str, machine: &str) -> Result<Secret, String> {
        let app = Caller {
            tier: Tier::App,
            client: ClientKind::App,
            peer: None,
            pid: None,
        };
        let reply = self
            .0
            .call(
                &app,
                ops::VAULT_ADMIN,
                &json!({"name": entry, "machine": machine}),
            )
            .map_err(|e| e.message)?;
        reply["secret"]
            .as_str()
            .map(Secret::new)
            .ok_or_else(|| "the Vault gave no password".to_string())
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub struct MemberTokens {
    members: Arc<dyn Admin>,
    tailscale: Tailscale,
    tailnet: Mutex<Option<(Instant, Option<String>)>>,
}

impl MemberTokens {
    pub fn new(members: Arc<dyn Admin>, tailscale: Tailscale) -> MemberTokens {
        MemberTokens {
            members,
            tailscale,
            tailnet: Mutex::new(None),
        }
    }

    fn member(&self, node: &str) -> Option<Value> {
        self.members.status()["members"]
            .as_array()?
            .iter()
            .find(|m| m["id"].as_str() == Some(node))
            .cloned()
    }
}

impl ClusterTokens for MemberTokens {
    fn join_token(&self) -> Result<String, String> {
        Err("A join token is for you to make, in Notchling".to_string())
    }

    fn cluster_id(&self) -> Result<Option<String>, String> {
        Ok(self.members.status()["cluster"]["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_string))
    }

    fn can_be_master(&self, _machine: &str) -> Option<bool> {
        None
    }

    fn can_be_master_of(&self, _machine: &str, node: Option<&str>) -> Option<bool> {
        self.member(node?)?["canBeMaster"].as_bool()
    }

    fn master_checks(&self, node: Option<&str>) -> Option<Checks> {
        checks_in(&self.members.status(), node?)
    }

    fn primary_addr(&self) -> Option<(String, u16)> {
        let status = self.members.status();
        let home = status["cluster"]["home"].as_str()?;
        let member = self.member(home)?;
        let addr = member["addrs"]
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .find(|ip| is_tailscale_ip(ip))?;
        let port = member["port"]
            .as_u64()
            .filter(|p| (1..=u64::from(u16::MAX)).contains(p))?;
        Some((addr.to_string(), port as u16))
    }

    fn tailnet(&self) -> Option<String> {
        let mut cached = self.tailnet.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, name)) = cached.as_ref() {
            if at.elapsed() < TAILNET_TTL {
                return name.clone();
            }
        }
        let name = self
            .tailscale
            .status()
            .ok()
            .map(|s| s.tailnet)
            .filter(|t| !t.is_empty());
        *cached = Some((Instant::now(), name.clone()));
        name
    }

    fn status_json(&self) -> Option<Value> {
        Some(self.members.status())
    }
}

pub fn build(wired: &Wired, layout: &Layout) -> Arc<Machines> {
    let roots = MachineRoots::in_data(&layout.data_dir);
    let registry = Arc::new(roots.registry());
    let runner: Arc<dyn Runner> = machine_runner(layout);
    let tailscale = Tailscale::new(runner.clone(), &default_cli());
    let router = Arc::new(Router::new(
        runner.clone(),
        tailscale.clone(),
        Arc::new(now_secs),
    ));
    let home = layout.home();
    let deps = Deps {
        runner: runner.clone(),
        secrets: Arc::new(NoSecrets),
        http: Arc::new(NoGitHub),
        opener: Arc::new(NoOpener),
        store: registry.clone(),
        mac: Arc::new(move || mac_facts::load(&home)),
        clock: Arc::new(now_secs),
        scratch: roots.scratch(),
        offers_data: Some(layout.data_dir.clone()),
        audit: Some(roots.audit_file()),
    };
    let vault: Arc<dyn OpGroup> = wired.vault.clone();
    let orgs = Organizations::new(&layout.data_dir).with_home(&layout.home());
    let members = Arc::new(LiveAdmin::new(
        wired.cluster.node.clone(),
        RegistryFiles {
            read_only: true,
            ..roots.registry()
        },
        orgs.clone(),
        runner.clone(),
    ));
    let mac_daemon = (!layout.fixture)
        .then(|| std::env::current_exe().ok())
        .flatten()
        .filter(|path| path.is_file());
    let engine = Arc::new(
        SetupEngine::new(deps, catalog::standard())
            .with_router(router)
            .with_admin_secrets(Arc::new(VaultAdmin(vault)))
            .with_cluster(
                Arc::new(MemberTokens::new(members.clone(), tailscale.clone())),
                Some(dist_dir(&layout.data_dir)),
            )
            .with_mac_daemon(mac_daemon)
            .with_downloads(Arc::new(CurlDownloads::new(runner)))
            .with_agent_limits(),
    );
    let on = wired.switches.machines.clone();
    let room = wired.acting.room.clone();
    Arc::new(Machines {
        engine,
        registry,
        orgs,
        tailscale,
        members,
        on: Arc::new(move || on.is_on()),
        consent: wired.consent.clone() as Arc<ConsentOps>,
        changed: Arc::new(move || {
            room.publish(topics::MACHINES, topics::MACHINES_CHANGED, &Value::Null)
        }),
    })
}
