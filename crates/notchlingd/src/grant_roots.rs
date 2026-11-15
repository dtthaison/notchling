use crate::vault_caller::Processes;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

const WALK_MAX: usize = 64;
const ENDED_MAX: usize = 512;
pub const MARKS_DIR: &str = "grant-marks";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn valid_job(job: &str) -> bool {
    job.strip_prefix("j-").is_some_and(|hex| {
        hex.len() == 12 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

#[derive(Default)]
struct Roots {
    live: BTreeMap<String, u32>,
    ended: BTreeSet<String>,
    order: VecDeque<String>,
}

#[derive(Default)]
pub struct JobRoots {
    inner: Mutex<Roots>,
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
        roots.live.insert(job.to_string(), pid);
    }

    pub fn end(&self, job: &str) {
        let mut roots = lock(&self.inner);
        roots.live.remove(job);
        if roots.ended.insert(job.to_string()) {
            roots.order.push_back(job.to_string());
        }
        while roots.order.len() > ENDED_MAX {
            if let Some(oldest) = roots.order.pop_front() {
                roots.ended.remove(&oldest);
            }
        }
    }

    pub fn is_ended(&self, job: &str) -> bool {
        lock(&self.inner).ended.contains(job)
    }

    pub fn rooted(&self) -> Vec<String> {
        lock(&self.inner).live.keys().cloned().collect()
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

pub struct GrantMarks {
    dir: PathBuf,
}

impl GrantMarks {
    pub fn new(cluster_dir: &Path) -> GrantMarks {
        GrantMarks {
            dir: cluster_dir.join(MARKS_DIR),
        }
    }

    pub fn mark(&self, job: &str) -> Result<(), String> {
        if !valid_job(job) {
            return Err("not a job id".to_string());
        }
        notchling_platform::files::ensure_private_dir(&self.dir)
            .map_err(|e| format!("the grant marks folder: {e}"))?;
        let path = self.dir.join(job);
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
            return Ok(());
        }
        notchling_platform::files::write_private_no_follow(&path, b"granted\n")
            .map_err(|e| format!("the grant mark for {job}: {e}"))
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

    pub fn remove(&self, job: &str) {
        if valid_job(job) {
            let _ = std::fs::remove_file(self.dir.join(job));
        }
    }
}
