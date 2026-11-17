use crate::acting_ops::now_ms;
use crate::git_args::fresh_args;
use crate::git_cache::{pick_reads, Budget, GitCache, READ_STOP_MS};
use crate::git_ops::{failed, lock, GitOps};
use crate::git_view::{Placed, Snapshot};
use notchling_engine::control::Launcher;
use notchling_engine::git::branch_list::{rows, ListInput, Work, WorktreeOf};
use notchling_engine::git::nearest::short_name;
use notchling_engine::git::reader::Reader;
use notchling_engine::git::refs::{Counts, Lines, RefRow};
use notchling_engine::git::worktrees::WorktreeEntry;
use notchling_engine::git::{read_branch_stat, Failure};
use notchling_local_api::ApiError;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const HEAD_CHARS: usize = 7;

struct Row {
    entry: WorktreeEntry,
    real: String,
    main: bool,
    current: bool,
    head: Option<String>,
    refrow: Option<RefRow>,
    base: Option<(String, Counts)>,
    unrelated: bool,
}

fn prefix_len(cwd: &Path, entry: &WorktreeEntry, real: &str) -> Option<usize> {
    [Path::new(&entry.path), Path::new(real)]
        .into_iter()
        .filter(|path| cwd.starts_with(path))
        .map(|path| path.components().count())
        .max()
}

fn absent(result: Result<Snapshot<'_>, Failure>) -> Result<Option<Snapshot<'_>>, ApiError> {
    match result {
        Ok(snap) => Ok(Some(snap)),
        Err(Failure::NotRepo) => Ok(None),
        Err(other) => Err(failed(other)),
    }
}

fn repo_name(snap: &Snapshot, key: &Path) -> String {
    let main = snap.listed.worktrees.first().filter(|entry| !entry.bare);
    let folder = match main {
        Some(entry) => Path::new(&entry.path).file_name(),
        None => key.file_name(),
    };
    let name = folder.map(|n| n.to_string_lossy().into_owned());
    match (main, name) {
        (None, Some(name)) => name.strip_suffix(".git").unwrap_or(&name).to_owned(),
        (_, Some(name)) => name,
        _ => String::new(),
    }
}

struct Job {
    real: String,
    base: String,
}

struct Reads {
    cache: Arc<Mutex<GitCache>>,
    key: PathBuf,
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
    pub(crate) fn worktrees(&self, cwd: &str, args: &Value) -> Result<Value, ApiError> {
        let start = now_ms();
        let reader = match Reader::open(cwd, self.launcher.as_ref()) {
            Ok(reader) => reader,
            Err(Failure::NotRepo) => return Ok(json!({"repo": false})),
            Err(other) => return Err(failed(other)),
        };
        let key = match reader.common_dir() {
            Ok(key) => key,
            Err(Failure::NotRepo) => return Ok(json!({"repo": false})),
            Err(other) => return Err(failed(other)),
        };
        let Some(snap) = absent(self.snapshot(&reader, &key, start))? else {
            return Ok(json!({"repo": false}));
        };
        let rows_in = self.rows_of(&snap, cwd);
        let jobs = self.start_reads(&snap, &key, &rows_in, args, start);
        self.wait_reads(&key, jobs, start);
        let work: HashMap<String, Work> = lock(&self.cache).repo(&key, now_ms()).work.clone();
        Ok(reply(&snap, &key, &rows_in, &work))
    }

    fn rows_of(&self, snap: &Snapshot, cwd: &str) -> Vec<Row> {
        let cwd = Path::new(cwd);
        let mut out: Vec<Row> = Vec::new();
        for (index, entry) in snap.listed.worktrees.iter().enumerate() {
            if entry.bare {
                continue;
            }
            let real = if entry.prunable {
                entry.path.clone()
            } else {
                std::fs::canonicalize(&entry.path)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| entry.path.clone())
            };
            let refrow = entry.branch.as_ref().and_then(|branch| {
                let name = format!("refs/heads/{branch}");
                snap.refs.iter().find(|row| row.refname == name).cloned()
            });
            let head = refrow
                .as_ref()
                .map(|row| row.commit.clone())
                .or_else(|| entry.head.clone());
            let mut row = Row {
                entry: entry.clone(),
                real,
                main: index == 0,
                current: false,
                head,
                refrow,
                base: None,
                unrelated: false,
            };
            if !row.entry.prunable {
                if let Some(head) = row.head.clone() {
                    let upstream = row.refrow.as_ref().and_then(|r| r.upstream.clone());
                    match snap.place(&head, row.entry.branch.as_deref(), upstream.as_deref()) {
                        Placed::Picked(candidate, counts) => {
                            row.base = Some((candidate.rev, counts));
                        }
                        Placed::Unrelated => row.unrelated = true,
                        Placed::Pending | Placed::Nothing => {}
                    }
                }
            }
            out.push(row);
        }
        let best = out
            .iter()
            .enumerate()
            .filter(|(_, row)| !row.entry.prunable)
            .filter_map(|(at, row)| prefix_len(cwd, &row.entry, &row.real).map(|len| (len, at)))
            .max_by_key(|(len, _)| *len)
            .map(|(_, at)| at);
        if let Some(at) = best {
            out[at].current = true;
        }
        out
    }

    fn start_reads(
        &self,
        snap: &Snapshot,
        key: &Path,
        rows: &[Row],
        args: &Value,
        start: i64,
    ) -> Vec<Job> {
        let readable: Vec<&Row> = rows
            .iter()
            .filter(|row| !row.current && !row.entry.prunable && row.base.is_some())
            .collect();
        let eligible: Vec<String> = readable.iter().map(|row| row.real.clone()).collect();
        let fresh: Vec<String> = fresh_args(args.get("fresh"), &snap.listed.worktrees)
            .into_iter()
            .map(|path: PathBuf| path.to_string_lossy().into_owned())
            .filter(|path| eligible.contains(path))
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

    fn wait_reads(&self, key: &Path, jobs: Vec<Job>, start: i64) {
        if jobs.is_empty() {
            return;
        }
        let (done, finished) = channel::<()>();
        let launcher = Arc::clone(&self.launcher);
        let mut reads = Reads {
            cache: Arc::clone(&self.cache),
            key: key.to_path_buf(),
            pending: jobs.iter().map(|job| job.real.clone()).collect(),
        };
        let spawned = std::thread::Builder::new()
            .name("git-worktree-reads".to_owned())
            .spawn(move || {
                let _done = done;
                let budget = Budget::new(start, READ_STOP_MS);
                read_work(launcher.as_ref(), &budget, jobs, &mut reads);
            });
        let left = (start + READ_STOP_MS - now_ms()).max(0) as u64;
        if spawned.is_ok() {
            let _ = finished.recv_timeout(Duration::from_millis(left));
        }
    }
}

fn base_json(row: &Row) -> (Value, Value, Value) {
    match (&row.base, row.entry.prunable) {
        (Some((rev, counts)), false) => (json!(rev), json!(counts.ahead), json!(counts.behind)),
        _ => (Value::Null, Value::Null, Value::Null),
    }
}

fn committed_at(snap: &Snapshot, row: &Row) -> Option<i64> {
    match &row.refrow {
        Some(found) => found.committed_ms,
        None => row
            .entry
            .head
            .as_ref()
            .and_then(|head| snap.facts.get(head))
            .and_then(|(time, _)| *time),
    }
}

fn worktree_json(snap: &Snapshot, row: &Row, work: &HashMap<String, Work>) -> Value {
    let (base, ahead, behind) = base_json(row);
    let track = row.refrow.as_ref();
    let shown_work = (!row.current && !row.entry.prunable)
        .then(|| work.get(&row.real))
        .flatten();
    json!({
        "path": row.entry.path,
        "real": row.real,
        "main": row.main,
        "current": row.current,
        "branch": row.entry.branch,
        "head": row.head.as_ref().map(|h| h.chars().take(HEAD_CHARS).collect::<String>()),
        "detached": row.entry.detached,
        "locked": row.entry.locked,
        "missing": row.entry.prunable,
        "upstream": track.and_then(|r| r.upstream.clone()),
        "upstream_ahead": track.map_or(0, |r| r.track.ahead),
        "upstream_behind": track.map_or(0, |r| r.track.behind),
        "upstream_gone": track.is_some_and(|r| r.track.gone),
        "base": base,
        "ahead": ahead,
        "behind": behind,
        "unrelated": row.unrelated && !row.entry.prunable,
        "committed_at": committed_at(snap, row),
        "work": shown_work,
    })
}

fn no_worktree_count(snap: &Snapshot, rows_in: &[Row]) -> u64 {
    let places: Vec<(WorktreeEntry, WorktreeOf)> = rows_in
        .iter()
        .map(|row| {
            (
                row.entry.clone(),
                WorktreeOf {
                    path: row.entry.path.clone(),
                    real: row.real.clone(),
                    main: row.main,
                    current: row.current,
                    missing: row.entry.prunable,
                    work: None,
                },
            )
        })
        .collect();
    let input = ListInput {
        refs: &snap.refs,
        worktrees: &places,
        remotes: &snap.listed.remotes,
        bases: &snap.bases,
        detached_facts: &snap.facts,
    };
    let counted = rows(&input, &|_, _| Some(Some(Counts::default())), &|_, _| {
        Some(Some(Lines::default()))
    });
    let names = snap.names();
    let default_short = snap
        .listed
        .default
        .as_deref()
        .map(|rev| short_name(rev, &names).to_owned());
    counted
        .iter()
        .filter(|row| row.worktree.is_none())
        .filter(|row| !(row.is_base && row.name == default_short))
        .count() as u64
}

fn reply(snap: &Snapshot, key: &Path, rows_in: &[Row], work: &HashMap<String, Work>) -> Value {
    json!({
        "repo": true,
        "name": repo_name(snap, key),
        "default": snap.listed.default,
        "integration": snap.bases.integration.as_ref().map(|c| c.rev.clone()),
        "fetched_at": snap.listed.fetched_at,
        "no_worktree": no_worktree_count(snap, rows_in),
        "truncated": snap.listed.truncated || snap.refs_truncated,
        "worktrees": rows_in
            .iter()
            .map(|row| worktree_json(snap, row, work))
            .collect::<Vec<Value>>(),
    })
}
