use notchling_engine::cluster::text::{plain, NAME_MAX};
use notchling_engine::cluster::NodeId;
use notchling_platform::files::{ensure_private_dir, write_private_no_follow};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

pub const WAITING_FILE: &str = "waiting.json";
const MAX_BYTES: u64 = 256 * 1024;
const MAX_ENTRIES: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waits {
    pub session: String,
    pub node: String,
    pub name: String,
}

fn entry_of(value: &Value) -> Option<Waits> {
    let text = |key: &str| value.get(key)?.as_str().map(str::to_string);
    Some(Waits {
        session: text("session")?,
        node: text("node")?,
        name: text("name")?,
    })
}

pub type Report = Arc<dyn Fn(&str) + Send + Sync>;

pub struct Waiting {
    cluster: PathBuf,
    report: Mutex<Option<Report>>,
    reported: Mutex<Option<String>>,
    serial: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Waiting {
    pub fn new(cluster: &Path) -> Arc<Waiting> {
        Arc::new(Waiting {
            cluster: cluster.to_path_buf(),
            report: Mutex::new(None),
            reported: Mutex::new(None),
            serial: Mutex::new(()),
        })
    }

    pub fn on_damage(&self, report: Report) {
        *lock(&self.report) = Some(report);
    }

    fn path(&self) -> PathBuf {
        self.cluster.join(WAITING_FILE)
    }

    fn damaged(&self, why: &str) {
        let first = {
            let mut reported = lock(&self.reported);
            let first = reported.is_none();
            if first {
                *reported = Some(why.to_string());
            }
            first
        };
        let report = lock(&self.report).clone();
        if let (true, Some(report)) = (first, report) {
            report(why);
        }
    }

    fn read(&self) -> Result<Vec<Waits>, String> {
        let path = self.path();
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };
        if !meta.is_file() || meta.len() > MAX_BYTES {
            return Err("it isn't a plain file of a sensible size".to_string());
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let file: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if file["version"] != 1 {
            return Err("its version isn't known".to_string());
        }
        file["waiting"]
            .as_array()
            .ok_or_else(|| "it holds no list".to_string())?
            .iter()
            .map(|v| entry_of(v).ok_or_else(|| "an entry is incomplete".to_string()))
            .collect()
    }

    pub fn list(&self) -> Vec<Waits> {
        match self.read() {
            Ok(list) => list,
            Err(why) => {
                self.damaged(&why);
                Vec::new()
            }
        }
    }

    pub fn gates(&self) -> BTreeMap<String, String> {
        self.list()
            .into_iter()
            .map(|w| (w.session, w.name))
            .collect()
    }

    pub fn get(&self, session: &str) -> Option<Waits> {
        self.list().into_iter().find(|w| w.session == session)
    }

    fn write(&self, waiting: Vec<Waits>) -> Result<(), String> {
        let rows: Vec<Value> = waiting
            .iter()
            .map(|w| json!({"session": w.session, "node": w.node, "name": w.name}))
            .collect();
        let bytes = serde_json::to_vec(&json!({"version": 1, "waiting": rows}))
            .map_err(|e| e.to_string())?;
        ensure_private_dir(&self.cluster).map_err(|e| e.to_string())?;
        write_private_no_follow(&self.path(), &bytes).map_err(|e| e.to_string())
    }

    fn set_aside(&self) {
        let path = self.path();
        let aside = path.with_extension("json.damaged");
        if !aside.exists() {
            let _ = std::fs::rename(&path, &aside);
        }
    }

    pub fn add(&self, session: &str, node: &NodeId, name: &str) -> Result<(), String> {
        let _serial = lock(&self.serial);
        let mut list = match self.read() {
            Ok(list) => list,
            Err(why) => {
                self.damaged(&why);
                self.set_aside();
                Vec::new()
            }
        };
        let entry = Waits {
            session: session.to_string(),
            node: node.as_str().to_string(),
            name: plain(name, NAME_MAX),
        };
        let at = list.iter().position(|w| w.session == session);
        match at {
            Some(i) => list[i] = entry,
            None if list.len() >= MAX_ENTRIES => {
                return Err("too many sessions are waiting".to_string())
            }
            None => list.push(entry),
        }
        self.write(list)
    }

    pub fn remove(&self, session: &str) -> Result<bool, String> {
        let _serial = lock(&self.serial);
        let mut list = match self.read() {
            Ok(list) => list,
            Err(why) => {
                self.damaged(&why);
                return Ok(false);
            }
        };
        let before = list.len();
        list.retain(|w| w.session != session);
        if list.len() == before {
            return Ok(false);
        }
        self.write(list).map(|()| true)
    }
}
