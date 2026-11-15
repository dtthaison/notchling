use crate::grant_roots::valid_key;
use notchling_engine::cluster::blob::sha256_of;
use notchling_engine::cluster::grant_files::GrantValue;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub const JOBS_FILE: &str = "jobs.json";
pub const MAX_JOBS: usize = 10_000;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn hash_of(token: &GrantValue) -> String {
    sha256_of(token.expose().as_bytes())
}

pub fn lost_name(session: &str) -> String {
    format!("login-{session}")
}

pub struct LoginJobs {
    path: PathBuf,
    map: Mutex<BTreeMap<String, String>>,
    readable: bool,
}

fn parse(text: &str) -> Option<BTreeMap<String, String>> {
    let value: Value = serde_json::from_str(text).ok()?;
    let jobs = value.get("jobs")?.as_object()?;
    jobs.iter()
        .map(|(session, hash)| Some((session.clone(), hash.as_str()?.to_string())))
        .collect()
}

impl LoginJobs {
    pub fn load(logins_dir: &Path) -> LoginJobs {
        let path = logins_dir.join(JOBS_FILE);
        let (map, readable) = match std::fs::read_to_string(&path) {
            Ok(text) => match parse(&text) {
                Some(map) => (map, true),
                None => (BTreeMap::new(), false),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (BTreeMap::new(), true),
            Err(_) => (BTreeMap::new(), false),
        };
        LoginJobs {
            path,
            map: Mutex::new(map),
            readable,
        }
    }

    pub fn usable(&self) -> bool {
        self.readable && lock(&self.map).len() < MAX_JOBS
    }

    pub fn all(&self) -> Vec<(String, String)> {
        lock(&self.map)
            .iter()
            .map(|(session, hash)| (session.clone(), hash.clone()))
            .collect()
    }

    pub fn record(&self, session: &str, hash: &str) -> Result<(), String> {
        if !valid_key(session) {
            return Err("a session key the record can't hold".to_string());
        }
        let mut map = lock(&self.map);
        map.insert(session.to_string(), hash.to_string());
        let jobs: serde_json::Map<String, Value> = map
            .iter()
            .map(|(session, hash)| (session.clone(), json!(hash)))
            .collect();
        let bytes = serde_json::to_vec(&json!({"v": 1, "jobs": jobs}))
            .map_err(|_| "the record can't be encoded".to_string())?;
        if let Some(dir) = self.path.parent() {
            notchling_platform::files::ensure_private_dir(dir).map_err(|e| e.to_string())?;
        }
        notchling_platform::files::write_private_no_follow(&self.path, &bytes)
            .map_err(|e| e.to_string())
    }
}
