use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const TTL: f64 = 1_200.0;
pub const MAX_RESERVED: usize = 32;
pub const MAX_AGENTS: usize = 64;
pub const MAX_FILE: u64 = 64 * 1024;
pub const KEEP_DAYS: f64 = 2.0;
pub const LOCK_WAIT_MS: u64 = 100;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reservation {
    pub id: String,
    pub ts: f64,
    pub agent: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub reserved: Vec<Reservation>,
    #[serde(default)]
    pub calls: BTreeMap<String, u64>,
}

impl Session {
    fn expire(&mut self, now: f64) {
        self.reserved.retain(|r| now - r.ts < TTL);
    }

    pub fn working(&self) -> usize {
        self.reserved.len()
    }

    pub fn reserve(&mut self, id: &str, now: f64, cap: u64) -> bool {
        self.expire(now);
        let others = self.reserved.iter().filter(|r| r.id != id).count() as u64;
        let over = others >= cap;
        match self.reserved.iter_mut().find(|r| r.id == id) {
            Some(known) => known.ts = now,
            None => self.reserved.push(Reservation {
                id: id.to_string(),
                ts: now,
                agent: None,
            }),
        }
        let extra = self.reserved.len().saturating_sub(MAX_RESERVED);
        self.reserved.drain(..extra);
        over
    }

    pub fn claim(&mut self, agent: &str, now: f64) {
        self.expire(now);
        if self
            .reserved
            .iter()
            .any(|r| r.agent.as_deref() == Some(agent))
        {
            return;
        }
        if let Some(open) = self.reserved.iter_mut().find(|r| r.agent.is_none()) {
            open.agent = Some(agent.to_string());
            open.ts = now;
        }
    }

    pub fn release(&mut self, agent: &str) {
        self.reserved.retain(|r| r.agent.as_deref() != Some(agent));
        self.calls.remove(agent);
    }

    pub fn count(&mut self, agent: &str, now: f64) -> u64 {
        if let Some(held) = self
            .reserved
            .iter_mut()
            .find(|r| r.agent.as_deref() == Some(agent))
        {
            held.ts = now;
        }
        if !self.calls.contains_key(agent) && self.calls.len() >= MAX_AGENTS {
            if let Some(first) = self.calls.keys().next().cloned() {
                self.calls.remove(&first);
            }
        }
        let n = self.calls.entry(agent.to_string()).or_insert(0);
        *n += 1;
        *n
    }
}

fn paths(dir: &Path, session: &str) -> (PathBuf, PathBuf) {
    let folder = dir.join("sessions");
    (
        folder.join(format!("{session}.json")),
        folder.join(format!("{session}.lock")),
    )
}

fn sweep(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let limit = std::time::Duration::from_secs_f64(KEEP_DAYS * 86_400.0);
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > limit);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn load(path: &Path) -> Session {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return Session::default();
    };
    let mut text = String::new();
    if file.take(MAX_FILE).read_to_string(&mut text).is_err() {
        return Session::default();
    }
    serde_json::from_str(&text).unwrap_or_default()
}

fn store(path: &Path, session: &Session) -> bool {
    let Ok(text) = serde_json::to_string(session) else {
        return false;
    };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path).is_ok()
}

#[cfg(unix)]
pub fn with_session<T>(
    dir: &Path,
    session: &str,
    change: impl FnOnce(&mut Session) -> T,
) -> Option<T> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    let (data, lock) = paths(dir, session);
    let folder = data.parent()?;
    std::fs::create_dir_all(folder).ok()?;
    let fresh = !data.exists();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&lock)
        .ok()?;
    let fd = file.as_raw_fd();
    let mut waited = 0;
    loop {
        if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            break;
        }
        if waited >= LOCK_WAIT_MS {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        waited += 5;
    }
    if fresh {
        sweep(folder);
    }
    let mut held = load(&data);
    let out = change(&mut held);
    store(&data, &held).then_some(out)
}

#[cfg(not(unix))]
pub fn with_session<T>(
    _dir: &Path,
    _session: &str,
    _change: impl FnOnce(&mut Session) -> T,
) -> Option<T> {
    None
}
