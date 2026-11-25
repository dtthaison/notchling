use crate::acting_ops::now_ms;
use crate::git_cache::{pick_reads, Budget, GitCache, READ_STOP_MS};
use crate::git_ops::{lock, GitOps};
use crate::git_worktrees::Row;
use notchling_engine::control::Launcher;
use notchling_engine::git::branch_list::Work;
use notchling_engine::git::read_branch_stat;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) struct Job {
    pub real: String,
    pub base: String,
}

struct Reads {
    cache: Arc<Mutex<GitCache>>,
    key: std::path::PathBuf,
    pending: Vec<String>,
}

impl Reads {
    fn done(&mut self, path: &str, work: Option<Work>) {
        self.pending.retain(|held| held != path);
        let mut cache = lock(&self.cache);
        let repo = cache.repo(&self.key, now_ms());
        if let Some(work) = work {
            repo.work.insert(path.to_owned(), work);
        }
        repo.reading.remove(path);
    }
}

impl Drop for Reads {
    fn drop(&mut self) {
        let mut cache = lock(&self.cache);
        let repo = cache.repo(&self.key, now_ms());
        for path in &self.pending {
            repo.reading.remove(path);
        }
    }
}

fn read_work(launcher: &dyn Launcher, budget: &Budget, jobs: Vec<Job>, reads: &mut Reads) {
    for job in jobs {
        let stat = budget
            .may_start(now_ms())
            .then(|| read_branch_stat(&job.real, Some(&job.base), launcher));
        let work = match stat {
            Some(Ok(stat)) => Some(Work {
                added: stat.added,
                removed: stat.removed,
                files: stat.file_count,
                uncommitted_files: stat.uncommitted_files,
                changed_at: stat.changed_at,
                read_at: now_ms(),
            }),
            _ => None,
        };
        reads.done(&job.real, work);
    }
}

impl GitOps {
    pub(crate) fn pick_jobs(
        &self,
        key: &Path,
        readable: &[&Row],
        fresh: &[String],
        start: i64,
    ) -> Vec<Job> {
        let eligible: Vec<String> = readable.iter().map(|row| row.real.clone()).collect();
        let fresh: Vec<String> = fresh
            .iter()
            .filter(|path| eligible.contains(path))
            .cloned()
            .collect();
        let picked = {
            let mut cache = lock(&self.cache);
            let repo = cache.repo(key, start);
            let picked = pick_reads(&eligible, &repo.work, &repo.reading, &fresh, now_ms());
            for path in &picked {
                repo.reading.insert(path.clone());
            }
            picked
        };
        picked
            .into_iter()
            .filter_map(|real| {
                let row = readable.iter().find(|row| row.real == real)?;
                let base = row.base.as_ref()?.0.clone();
                Some(Job { real, base })
            })
            .collect()
    }

    pub(crate) fn spawn_reads(
        &self,
        key: &Path,
        jobs: Vec<Job>,
        start: i64,
    ) -> Option<Receiver<()>> {
        if jobs.is_empty() {
            return None;
        }
        let (done, finished) = channel::<()>();
        let launcher = Arc::clone(&self.launcher);
        let mut reads = Reads {
            cache: Arc::clone(&self.cache),
            key: key.to_path_buf(),
            pending: jobs.iter().map(|job| job.real.clone()).collect(),
        };
        std::thread::Builder::new()
            .name("git-worktree-reads".to_owned())
            .spawn(move || {
                let _done = done;
                let budget = Budget::new(start, READ_STOP_MS);
                read_work(launcher.as_ref(), &budget, jobs, &mut reads);
            })
            .ok()
            .map(|_| finished)
    }
}

pub(crate) fn await_reads(finished: Option<Receiver<()>>, start: i64) {
    if let Some(finished) = finished {
        let left = (start + READ_STOP_MS - now_ms()).max(0) as u64;
        let _ = finished.recv_timeout(Duration::from_millis(left));
    }
}
