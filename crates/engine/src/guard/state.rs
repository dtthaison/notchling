use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const TTL: f64 = 1_200.0;
pub const PENDING_TTL: f64 = 120.0;
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
pub struct Call {
    pub n: u64,
    pub ts: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub reserved: Vec<Reservation>,
    #[serde(default)]
    pub calls: BTreeMap<String, Call>,
}

impl Session {
    fn expire(&mut self, now: f64) {
        self.reserved.retain(|r| {
            let limit = if r.agent.is_some() { TTL } else { PENDING_TTL };
            now - r.ts < limit
        });
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

    pub fn claim(&mut self, agent: &str, now: f64, id: Option<&str>) {
        self.expire(now);
        if self
            .reserved
            .iter()
            .any(|r| r.agent.as_deref() == Some(agent))
        {
            return;
        }
        let by_id = id.and_then(|id| {
            self.reserved
                .iter()
                .position(|r| r.id == id && r.agent.is_none())
        });
        let slot = by_id.or_else(|| {
            if id.is_some_and(|id| self.reserved.iter().any(|r| r.id == id)) {
                return None;
            }
            self.reserved.iter().position(|r| r.agent.is_none())
        });
        if let Some(open) = slot.and_then(|i| self.reserved.get_mut(i)) {
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
            let oldest = self
                .calls
                .iter()
                .min_by(|a, b| a.1.ts.total_cmp(&b.1.ts))
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                self.calls.remove(&oldest);
            }
        }
        let call = self.calls.entry(agent.to_string()).or_default();
        call.n += 1;
        call.ts = now;
        call.n
    }
}

fn paths(dir: &Path, session: &str) -> (PathBuf, PathBuf) {
    let folder = dir.join("sessions");
    (
        folder.join(format!("{session}.json")),
        folder.join(format!("{session}.lock")),
    )
}

fn age_of(path: &Path) -> Option<std::time::Duration> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
}

fn expired(data: &Path, lock: &Path, limit: std::time::Duration) -> bool {
    age_of(data)
        .or_else(|| age_of(lock))
        .is_some_and(|age| age > limit)
}

#[cfg(unix)]
fn sweep_one(folder: &Path, stem: &str, limit: std::time::Duration) {
    let data = folder.join(format!("{stem}.json"));
    let lock = folder.join(format!("{stem}.lock"));
    if !expired(&data, &lock, limit) {
        return;
    }
    let Some(_held) = open_locked_within(&lock, 0) else {
        return;
    };
    if !expired(&data, &lock, limit) {
        return;
    }
    let _ = std::fs::remove_file(&data);
    let _ = std::fs::remove_file(folder.join(format!("{stem}.json.tmp")));
    let _ = std::fs::remove_file(&lock);
}

#[cfg(unix)]
fn sweep(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let limit = std::time::Duration::from_secs_f64(KEEP_DAYS * 86_400.0);
    let mut stems: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            [".json.tmp", ".json", ".lock"]
                .iter()
                .find_map(|ext| name.strip_suffix(ext).map(str::to_string))
        })
        .collect();
    stems.sort();
    stems.dedup();
    for stem in stems {
        sweep_one(folder, &stem, limit);
    }
}

#[cfg(unix)]
fn lock_now(file: &std::fs::File) -> bool {
    use std::os::unix::io::AsRawFd;
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
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
fn open_locked(lock: &Path) -> Option<std::fs::File> {
    open_locked_within(lock, LOCK_WAIT_MS)
}

#[cfg(unix)]
fn open_locked_within(lock: &Path, wait_ms: u64) -> Option<std::fs::File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    for _ in 0..3 {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(lock)
            .ok()?;
        let mut waited = 0;
        while !lock_now(&file) {
            if waited >= wait_ms {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            waited += 5;
        }
        let same = match (file.metadata(), std::fs::metadata(lock)) {
            (Ok(held), Ok(named)) => held.ino() == named.ino() && held.dev() == named.dev(),
            _ => false,
        };
        if same {
            return Some(file);
        }
    }
    None
}

#[cfg(unix)]
pub fn with_session<T>(
    dir: &Path,
    session: &str,
    change: impl FnOnce(&mut Session) -> T,
) -> Option<T> {
    let (data, lock) = paths(dir, session);
    let folder = data.parent()?;
    std::fs::create_dir_all(folder).ok()?;
    let fresh = !data.exists();
    let _held = open_locked(&lock)?;
    if fresh {
        sweep(folder);
    }
    let mut state = load(&data);
    let out = change(&mut state);
    store(&data, &state).then_some(out)
}

#[cfg(not(unix))]
pub fn with_session<T>(
    _dir: &Path,
    _session: &str,
    _change: impl FnOnce(&mut Session) -> T,
) -> Option<T> {
    None
}
