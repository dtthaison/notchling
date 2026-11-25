use crate::acting_ops::Room;
use crate::cluster_node::ClusterNode;
use notchling_engine::cluster::failover::Failover;
use notchling_engine::cluster::LocalCmd;
use notchling_engine::settings::Settings;
use notchling_local_api::topics;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const KEY: &str = "clusterFailover";

pub fn parse(value: &Value) -> Option<Failover> {
    serde_json::from_value(value.clone()).ok()
}

pub trait FailoverSink: Send + Sync {
    fn set(&self, value: Failover) -> Result<(), String>;
}

pub const STARTING: &str = "The cluster is still starting. Try again in a moment.";

pub trait SinkNode: Send + Sync {
    fn started(&self) -> bool;
    fn in_cluster(&self) -> bool;
    fn journal(&self, value: Failover) -> Result<(), String>;
}

impl SinkNode for ClusterNode {
    fn started(&self) -> bool {
        ClusterNode::started(self)
    }

    fn in_cluster(&self) -> bool {
        !self.status()["cluster"].is_null()
    }

    fn journal(&self, value: Failover) -> Result<(), String> {
        self.local(LocalCmd::SetFailover { failover: value })
            .map(|_| ())
            .map_err(|e| e.message)
    }
}

pub struct NodeSink {
    node: Arc<dyn SinkNode>,
}

impl NodeSink {
    pub fn new(node: Arc<ClusterNode>) -> NodeSink {
        NodeSink { node }
    }

    pub fn over(node: Arc<dyn SinkNode>) -> NodeSink {
        NodeSink { node }
    }
}

impl FailoverSink for NodeSink {
    fn set(&self, value: Failover) -> Result<(), String> {
        if !self.node.started() {
            return Err(STARTING.to_string());
        }
        if !self.node.in_cluster() {
            return Ok(());
        }
        self.node.journal(value)
    }
}

static ORDER: AtomicU64 = AtomicU64::new(0);

pub fn next_order() -> u64 {
    ORDER.fetch_add(1, Ordering::SeqCst) + 1
}

pub struct KeyMirror {
    file: PathBuf,
    room: Arc<dyn Room>,
    last: Mutex<u64>,
}

impl KeyMirror {
    pub fn new(file: PathBuf, room: Arc<dyn Room>) -> KeyMirror {
        KeyMirror {
            file,
            room,
            last: Mutex::new(0),
        }
    }

    pub fn follow(&self, value: Failover, order: u64) {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if order < *last {
            return;
        }
        *last = order;
        let Ok(mut store) = Settings::open(&self.file) else {
            return;
        };
        let value = json!(value);
        if store.get(KEY).as_ref() == Some(&value) {
            return;
        }
        if store.set(KEY, value.clone()).is_err() {
            return;
        }
        let event = json!({"key": KEY, "value": value, "by": "cluster"});
        self.room
            .publish(topics::SETTINGS, topics::SETTINGS_CHANGED, &event);
    }
}

#[cfg(test)]
#[path = "cluster_failover_setting_tests.rs"]
mod tests;
