use crate::shell_jobs::SESSION_PREFIX;
use crate::vault_caller::Processes;
use notchling_engine::cluster::core::{ATTACH_CALL_MS, CALL_MS};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

const WALK_MAX: usize = 64;
const ENDED_MAX: usize = 512;
const BEGUN_MAX: usize = 512;
const REVOKES_MAX: usize = 512;
pub const PENDING_MS: i64 = (ATTACH_CALL_MS + CALL_MS) as i64;
pub const MARKS_DIR: &str = "grant-marks";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn valid_job(job: &str) -> bool {
    job.strip_prefix("j-").is_some_and(|hex| {
        hex.len() == 12 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartTool {
    Claude,
    Agy,
}

#[derive(Default)]
struct Begun {
    keys: Vec<String>,
    claude: bool,
}

#[derive(Default)]
struct Roots {
    live: BTreeMap<String, u32>,
    begun: BTreeMap<String, Begun>,
    begun_order: VecDeque<String>,
    pending: BTreeMap<String, i64>,
    ended: BTreeSet<String>,
    order: VecDeque<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Local {
    pub live: BTreeMap<String, Vec<String>>,
    pub pending: BTreeSet<String>,
}

type Revoke = (u64, String, Option<BTreeSet<String>>);

#[derive(Default)]
pub struct JobRoots {
    inner: Mutex<Roots>,
    gate: Mutex<()>,
    seq: AtomicU64,
    revokes: Mutex<VecDeque<Revoke>>,
}

impl JobRoots {
    pub fn new() -> JobRoots {
        JobRoots::default()
    }

    pub fn set(&self, job: &str, pid: u32) {
        if pid <= 1 || !valid_job(job) {
            return;
        }
        let mut roots = lock(&self.inner);
        if roots.ended.contains(job) {
            return;
        }
        roots.pending.remove(job);
        roots.live.insert(job.to_string(), pid);
    }

    pub fn begin(&self, job: &str, tool: StartTool) -> bool {
        if !valid_job(job) {
            return false;
        }
        let mut roots = lock(&self.inner);
        if roots.ended.contains(job) {
            return false;
        }
        roots.pending.remove(job);
        if !roots.begun.contains_key(job) {
            roots.begun.insert(job.to_string(), Begun::default());
            roots.begun_order.push_back(job.to_string());
        }
        if let Some(begun) = roots.begun.get_mut(job) {
            begun.claude |= tool == StartTool::Claude;
        }
        while roots.begun_order.len() > BEGUN_MAX {
            if let Some(oldest) = roots.begun_order.pop_front() {
                roots.begun.remove(&oldest);
            }
        }
        true
    }

    pub fn note_session(&self, job: &str, key: &str) {
        if !valid_key(key) {
            return;
        }
        let mut roots = lock(&self.inner);
        if let Some(begun) = roots.begun.get_mut(job) {
            if !begun.keys.iter().any(|k| k == key) {
                begun.keys.push(key.to_string());
            }
        }
    }

    pub fn ran_claude(&self, job: &str) -> bool {
        lock(&self.inner).begun.get(job).is_some_and(|b| b.claude)
    }

    pub fn is_live(&self, job: &str) -> bool {
        let roots = lock(&self.inner);
        roots.live.contains_key(job) || roots.begun.contains_key(job)
    }

    pub fn hold(&self, job: &str, now_ms: i64) {
        if !valid_job(job) {
            return;
        }
        let mut roots = lock(&self.inner);
        if roots.ended.contains(job)
            || roots.live.contains_key(job)
            || roots.begun.contains_key(job)
        {
            return;
        }
        roots
            .pending
            .entry(job.to_string())
            .and_modify(|since| {
                if now_ms.saturating_sub(*since) >= PENDING_MS {
                    *since = now_ms;
                }
            })
            .or_insert(now_ms);
    }

    pub fn release_pending(&self, job: &str) {
        lock(&self.inner).pending.remove(job);
    }

    pub fn is_pending(&self, job: &str, now_ms: i64) -> bool {
        lock(&self.inner)
            .pending
            .get(job)
            .is_some_and(|since| now_ms.saturating_sub(*since) < PENDING_MS)
    }

    pub fn local(&self, now_ms: i64) -> Local {
        let mut roots = lock(&self.inner);
        roots
            .pending
            .retain(|_, since| now_ms.saturating_sub(*since) < PENDING_MS);
        let mut live: BTreeMap<String, Vec<String>> = roots
            .begun
            .iter()
            .map(|(job, begun)| (job.clone(), begun.keys.clone()))
            .collect();
        for job in roots.live.keys() {
            live.entry(job.clone())
                .or_default()
                .push(format!("{SESSION_PREFIX}{job}"));
        }
        Local {
            live,
            pending: roots.pending.keys().cloned().collect(),
        }
    }

    pub fn keys(&self, job: &str) -> Vec<String> {
        let roots = lock(&self.inner);
        let mut keys = roots
            .begun
            .get(job)
            .map(|b| b.keys.clone())
            .unwrap_or_default();
        if roots.live.contains_key(job) {
            keys.push(format!("{SESSION_PREFIX}{job}"));
        }
        keys
    }

    pub fn end(&self, job: &str) {
        let mut roots = lock(&self.inner);
        roots.live.remove(job);
        if roots.begun.remove(job).is_some() {
            roots.begun_order.retain(|j| j != job);
        }
        roots.pending.remove(job);
        if roots.ended.insert(job.to_string()) {
            roots.order.push_back(job.to_string());
        }
        while roots.order.len() > ENDED_MAX {
            if let Some(oldest) = roots.order.pop_front() {
                roots.ended.remove(&oldest);
            }
        }
    }

    pub fn gate(&self) -> MutexGuard<'_, ()> {
        lock(&self.gate)
    }

    pub fn now_seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    pub fn note_revoke(&self, job: &str, names: Option<&[String]>) {
        let at = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let names = names.map(|list| list.iter().map(|n| n.to_lowercase()).collect());
        let mut book = lock(&self.revokes);
        book.push_back((at, job.to_string(), names));
        while book.len() > REVOKES_MAX {
            book.pop_front();
        }
    }

    pub fn revoked_since(&self, job: &str, name: &str, since: u64) -> bool {
        lock(&self.revokes).iter().any(|(at, j, names)| {
            *at > since && j == job && names.as_ref().is_none_or(|n| n.contains(name))
        })
    }

    pub fn is_ended(&self, job: &str) -> bool {
        lock(&self.inner).ended.contains(job)
    }

    pub fn rooted(&self) -> Vec<String> {
        let roots = lock(&self.inner);
        roots
            .live
            .keys()
            .chain(roots.begun.keys())
            .cloned()
            .collect()
    }

    pub fn job_of(&self, procs: &dyn Processes, pid: u32) -> Option<String> {
        let roots = lock(&self.inner);
        if roots.live.is_empty() {
            return None;
        }
        let mut seen = HashSet::new();
        let mut at = Some(pid);
        for _ in 0..WALK_MAX {
            let current = at.filter(|p| *p > 1 && seen.insert(*p))?;
            if let Some((job, _)) = roots.live.iter().find(|(_, root)| **root == current) {
                return Some(job.clone());
            }
            at = procs.parent(current);
        }
        None
    }
}

pub const KEY_MAX: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marked {
    pub job: String,
    pub keys: Vec<String>,
    pub ended_ms: Option<i64>,
}

pub fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= KEY_MAX && !key.chars().any(char::is_control)
}

pub struct GrantMarks {
    dir: PathBuf,
}

fn parse(text: &str) -> (Vec<String>, Option<i64>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return (Vec::new(), None);
    };
    let keys = value
        .get("keys")
        .and_then(|k| k.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|k| k.as_str())
                .filter(|k| valid_key(k))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    (keys, value.get("endedMs").and_then(|t| t.as_i64()))
}

impl GrantMarks {
    pub fn new(cluster_dir: &Path) -> GrantMarks {
        GrantMarks {
            dir: cluster_dir.join(MARKS_DIR),
        }
    }

    fn read(&self, job: &str) -> Option<(Vec<String>, Option<i64>)> {
        let path = self.dir.join(job);
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
            return None;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        Some(parse(&text))
    }

    fn write(&self, job: &str, keys: &[String], ended: Option<i64>) -> Result<(), String> {
        notchling_platform::files::ensure_private_dir(&self.dir)
            .map_err(|e| format!("the grant marks folder: {e}"))?;
        let mut body = serde_json::json!({"keys": keys});
        if let Some(at) = ended {
            body["endedMs"] = serde_json::json!(at);
        }
        let text = format!("{body}\n");
        notchling_platform::files::write_private_no_follow(&self.dir.join(job), text.as_bytes())
            .map_err(|e| format!("the grant mark for {job}: {e}"))
    }

    #[cfg(test)]
    pub fn mark(&self, job: &str) -> Result<(), String> {
        self.mark_with(job, &[])
    }

    pub fn mark_with(&self, job: &str, keys: &[String]) -> Result<(), String> {
        if !valid_job(job) {
            return Err("not a job id".to_string());
        }
        if keys.iter().any(|k| !valid_key(k)) {
            return Err("a session key the mark can't hold".to_string());
        }
        let (mut all, ended) = self.read(job).unwrap_or_default();
        let known = all.len();
        for key in keys {
            if !all.contains(key) {
                all.push(key.clone());
            }
        }
        if self.read(job).is_some() && all.len() == known {
            return Ok(());
        }
        self.write(job, &all, ended)
    }

    pub fn stamp_end(&self, job: &str, now_ms: i64) -> Result<(), String> {
        let (keys, ended) = self
            .read(job)
            .ok_or_else(|| format!("no grant mark for {job}"))?;
        if ended.is_some() {
            return Ok(());
        }
        self.write(job, &keys, Some(now_ms))
    }

    pub fn marked(&self) -> Vec<Marked> {
        self.list()
            .into_iter()
            .filter_map(|job| {
                let (keys, ended_ms) = self.read(&job)?;
                Some(Marked {
                    job,
                    keys,
                    ended_ms,
                })
            })
            .collect()
    }

    pub fn list(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| valid_job(name))
            .collect()
    }

    #[cfg(test)]
    pub fn remove(&self, job: &str) {
        if valid_job(job) {
            let _ = std::fs::remove_file(self.dir.join(job));
        }
    }
}

#[cfg(test)]
#[path = "grant_marks_tests.rs"]
mod tests;
