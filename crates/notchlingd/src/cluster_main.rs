use crate::cluster_node::ClusterNode;
use crate::cluster_sessions::LocalRows;
use notchling_engine::cluster::LocalCmd;
use notchling_engine::sessions::Agent;
use serde_json::Value;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const EVERY: Duration = Duration::from_secs(2);

pub fn is_home(status: &Value) -> bool {
    let home = status
        .get("cluster")
        .and_then(|c| c.get("home"))
        .and_then(Value::as_str);
    home.is_some() && home == status.get("me").and_then(Value::as_str)
}

pub fn agents_of(rows: Vec<Value>) -> Vec<Agent> {
    rows.into_iter()
        .filter_map(|row| serde_json::from_value(row).ok())
        .collect()
}

pub fn tick(node: &ClusterNode, rows: &dyn LocalRows) -> bool {
    if !is_home(&node.status()) {
        return false;
    }
    let Ok(rows) = rows.rows() else {
        return false;
    };
    node.local(LocalCmd::MainSeen {
        agents: agents_of(rows),
    })
    .is_ok()
}

pub struct MainFollower {
    stop: Mutex<Option<Sender<()>>>,
}

impl MainFollower {
    pub fn spawn(
        node: Arc<ClusterNode>,
        rows: Arc<dyn LocalRows>,
        every: Duration,
    ) -> MainFollower {
        let (stop, stopped) = channel::<()>();
        let spawned = std::thread::Builder::new()
            .name("cluster-main".into())
            .spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(every) {
                    tick(&node, rows.as_ref());
                }
            });
        MainFollower {
            stop: Mutex::new(spawned.ok().map(|_| stop)),
        }
    }

    pub fn close(&self) {
        self.stop.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}

#[cfg(test)]
#[path = "cluster_main_tests.rs"]
mod tests;
