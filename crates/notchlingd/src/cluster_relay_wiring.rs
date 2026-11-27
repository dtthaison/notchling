use crate::acting_ops::ActingOps;
use crate::bridge_backend::model_id;
use crate::bridge_cluster::PhoneCluster;
use crate::bridge_ops::BridgeOps;
use crate::cluster_move::{Acts, LateLink, Link};
use crate::cluster_node::wire_error;
use crate::cluster_relay::{HeldRelay, Limits, Parts, SetView, Wire};
use notchling_engine::bridge::net::devices::Replica;
use notchling_engine::cluster::main_sessions::MainSession;
use notchling_engine::cluster::snapshot::plan::bridge_devices;
use notchling_engine::cluster::standby_store::StandbyStore;
use notchling_engine::cluster::wire::{Origin, OriginTier, WireError};
use notchling_engine::cluster::{LocalCmd, NodeId};
use notchling_engine::sessions::Agent;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub type HeldSlot = Arc<OnceLock<Arc<HeldRelay>>>;

pub struct NodeWire {
    pub link: Arc<LateLink>,
    pub phone: Arc<PhoneCluster>,
}

impl Wire for NodeWire {
    fn status(&self) -> Value {
        self.link.status()
    }

    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        within: Duration,
    ) -> Result<Value, WireError> {
        let status = self.link.status();
        let me = status["me"].as_str().and_then(NodeId::parse);
        if me.as_ref() == Some(to) {
            return self.phone.call(op, &args).map_err(wire_error);
        }
        let node = me.ok_or_else(|| WireError::new("no_cluster", "this node is in no cluster"))?;
        self.link.local(
            LocalCmd::Forward {
                node: to.clone(),
                op: op.to_string(),
                args: args.into(),
                origin: Origin {
                    node,
                    tier: OriginTier::App,
                },
            },
            within,
        )
    }
}

pub struct StoreView {
    pub cluster: PathBuf,
    pub link: Arc<LateLink>,
}

impl StoreView {
    fn store(&self) -> Option<StandbyStore> {
        let status = self.link.status();
        let home = NodeId::parse(status["cluster"]["home"].as_str()?)?;
        Some(StandbyStore::new(&self.cluster, &home))
    }
}

impl SetView for StoreView {
    fn newest_ms(&self) -> Option<i64> {
        self.store()?.current().map(|m| m.created_ms)
    }

    fn transcript(&self, original: &Path) -> Option<PathBuf> {
        let store = self.store()?;
        store
            .stored_tail(original)
            .or_else(|| {
                let set = store.current()?.set;
                store.stored(&set, original)
            })
            .filter(|path| path.is_file())
    }
}

pub struct HeldActs {
    pub inner: Arc<dyn Acts>,
    pub slot: HeldSlot,
}

impl Acts for HeldActs {
    fn stop_claude(&self, id: &str) -> Result<(), String> {
        self.inner.stop_claude(id)
    }

    fn stop_codex(&self, id: &str) {
        self.inner.stop_codex(id)
    }

    fn start_claude(&self, session: &MainSession) -> Result<(), String> {
        self.inner.start_claude(session)
    }

    fn primary_moved(&self, to: &NodeId) {
        self.inner.primary_moved(to)
    }

    fn db_replaced(&self) {
        if let Some(held) = self.slot.get() {
            held.carry();
        }
    }
}

pub fn devices_replica(cluster: PathBuf, data: PathBuf, link: Arc<LateLink>) -> Replica {
    Arc::new(move || {
        let status = link.status();
        let home = NodeId::parse(status["cluster"]["home"].as_str()?)?;
        if status["me"].as_str() == Some(home.as_str()) {
            return None;
        }
        let store = StandbyStore::new(&cluster, &home);
        let set = store.current()?.set;
        store
            .stored(&set, &bridge_devices(&data))
            .filter(|path| path.is_file())
    })
}

pub struct RelayEnv {
    pub cluster: PathBuf,
    pub link: Arc<LateLink>,
    pub phone: Arc<PhoneCluster>,
    pub bridge: Arc<BridgeOps>,
    pub acting: Arc<ActingOps>,
    pub sessions: Arc<Mutex<Value>>,
    pub clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

fn local_rows(sessions: &Mutex<Value>, me: Option<&str>) -> Vec<String> {
    let Some(me) = me else {
        return Vec::new();
    };
    let list = sessions.lock().unwrap_or_else(|e| e.into_inner()).clone();
    serde_json::from_value::<Vec<Agent>>(list)
        .unwrap_or_default()
        .into_iter()
        .map(|agent| format!("{me}/{}", agent.id))
        .collect()
}

pub fn parts(env: RelayEnv) -> Parts {
    let (rows_phone, rows_link, rows_sessions) =
        (env.phone.clone(), env.link.clone(), env.sessions.clone());
    let prompts_phone = env.phone.clone();
    let acting = env.acting.clone();
    let bridge = env.bridge.clone();
    Parts {
        wire: Arc::new(NodeWire {
            link: env.link.clone(),
            phone: env.phone.clone(),
        }),
        dir: env.cluster.clone(),
        clock: env.clock,
        prompts: Arc::new(move |id| prompts_phone.prompts().into_iter().find(|p| p.id == id)),
        model: Arc::new(move |short| model_id(short, acting.catalog())),
        rows: Arc::new(move || {
            let status = rows_link.status();
            let me = status["me"].as_str();
            let mut rows: Vec<String> = rows_phone.agents().into_iter().map(|a| a.id).collect();
            rows.extend(local_rows(&rows_sessions, me));
            rows
        }),
        set: Arc::new(StoreView {
            cluster: env.cluster,
            link: env.link,
        }),
        bridge: Arc::new(move || bridge.served()),
        limits: Limits::DEFAULT,
    }
}
