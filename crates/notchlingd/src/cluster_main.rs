use crate::cluster_node::ClusterNode;
use crate::cluster_sessions::LocalRows;
use notchling_engine::cluster::main_sessions::tool_of_row;
use notchling_engine::cluster::{LocalCmd, Tool};
use notchling_engine::control::session::flags_for_job;
use notchling_engine::sessions::{Agent, AgentTarget, Roots};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::path::Path;
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

pub const FIRST_LINE_MAX: usize = 64 * 1024;
const EXEC: &str = "codex_exec";

pub fn first_line(path: &Path) -> Option<String> {
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(FIRST_LINE_MAX as u64)
        .read_to_end(&mut buf)
        .ok()?;
    let end = buf.iter().position(|b| *b == b'\n')?;
    String::from_utf8(buf[..end].to_vec()).ok()
}

pub fn originator(line: &str) -> Option<String> {
    let meta: Value = serde_json::from_str(line).ok()?;
    if meta.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    meta.get("payload")?
        .get("originator")?
        .as_str()
        .map(str::to_string)
}

#[derive(Default)]
pub struct Originators {
    seen: Mutex<HashMap<String, bool>>,
    roots: Option<Roots>,
}

impl Originators {
    pub fn with_roots(roots: Roots) -> Originators {
        Originators {
            seen: Mutex::new(HashMap::new()),
            roots: Some(roots),
        }
    }

    pub fn respawn_flags(&self, agents: &[Agent]) -> BTreeMap<String, Vec<String>> {
        let Some(roots) = &self.roots else {
            return BTreeMap::new();
        };
        agents
            .iter()
            .filter(|a| a.node.is_none() && tool_of_row(&a.id) == Tool::Claude)
            .filter_map(|a| match &a.target {
                AgentTarget::Job(job) => Some((a.id.clone(), flags_for_job(roots, job, &a.name))),
                _ => None,
            })
            .collect()
    }

    pub fn exec_ids(&self, agents: &[Agent]) -> BTreeSet<String> {
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        let live: BTreeSet<&str> = agents
            .iter()
            .filter_map(|a| a.transcript_path.as_deref())
            .collect();
        seen.retain(|path, _| live.contains(path.as_str()));
        let mut ids = BTreeSet::new();
        for agent in agents.iter().filter(|a| a.id.starts_with("codex:")) {
            let Some(path) = agent.transcript_path.as_deref() else {
                continue;
            };
            let known = match seen.get(path) {
                Some(known) => Some(*known),
                None => first_line(Path::new(path))
                    .and_then(|line| originator(&line))
                    .map(|name| {
                        seen.insert(path.to_string(), name == EXEC);
                        name == EXEC
                    }),
            };
            if known == Some(true) {
                ids.insert(agent.id.clone());
            }
        }
        ids
    }
}

pub fn tick(node: &ClusterNode, rows: &dyn LocalRows, origins: &Originators) -> bool {
    if !is_home(&node.status()) {
        return false;
    }
    let Ok(Some(rows)) = rows.complete_rows() else {
        return false;
    };
    let agents = agents_of(rows);
    let codex_exec = origins.exec_ids(&agents);
    let respawn = origins.respawn_flags(&agents);
    node.local(LocalCmd::MainSeen {
        agents,
        codex_exec,
        respawn,
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
        MainFollower::spawn_with(node, rows, every, Originators::default())
    }

    pub fn spawn_with(
        node: Arc<ClusterNode>,
        rows: Arc<dyn LocalRows>,
        every: Duration,
        origins: Originators,
    ) -> MainFollower {
        let (stop, stopped) = channel::<()>();
        let spawned = std::thread::Builder::new()
            .name("cluster-main".into())
            .spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(every) {
                    tick(&node, rows.as_ref(), &origins);
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
