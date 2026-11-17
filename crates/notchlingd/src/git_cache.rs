use crate::git_args::FRESH_READS;
use notchling_engine::git::branch_list::Work;
use notchling_engine::git::refs::{Counts, Lines};
use notchling_engine::git::worktrees::WorktreeEntry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const NUMBERS_CAP: usize = 2_000;
pub const IDLE_DROP_MS: i64 = 600_000;
pub const LIST_FOR_MS: i64 = 5_000;
pub const STALE_MS: i64 = 15_000;
pub const MISSING_READS: usize = 4;
pub const READ_STOP_MS: i64 = 1_500;
pub const LINES_BUDGET_MS: i64 = 1_000;
pub const SLOW_EVERY_MS: i64 = 15_000;
pub const PENDING_AGAIN_MS: i64 = 2_000;

type Pair = (String, String);

#[derive(Default)]
struct Entry {
    counts: Option<Option<Counts>>,
    lines: Option<Lines>,
    related: bool,
    unrelated: bool,
    stamp: u64,
}

#[derive(Default)]
pub struct Numbers {
    entries: HashMap<Pair, Entry>,
    order: BTreeMap<u64, Pair>,
    clock: u64,
}

fn pair(base: &str, head: &str) -> Pair {
    (base.to_string(), head.to_string())
}

impl Numbers {
    fn touch(&mut self, key: &Pair) -> Option<&mut Entry> {
        self.clock += 1;
        let stamp = self.clock;
        let entry = self.entries.get_mut(key)?;
        self.order.remove(&entry.stamp);
        entry.stamp = stamp;
        self.order.insert(stamp, key.clone());
        Some(entry)
    }

    fn entry(&mut self, key: Pair) -> &mut Entry {
        self.clock += 1;
        let stamp = self.clock;
        let old = self.entries.entry(key.clone()).or_default();
        self.order.remove(&old.stamp);
        old.stamp = stamp;
        self.order.insert(stamp, key.clone());
        while self.entries.len() > NUMBERS_CAP {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.entries.entry(key).or_default()
    }

    pub fn counts(&mut self, base: &str, head: &str) -> Option<Option<Counts>> {
        let entry = self.touch(&pair(base, head))?;
        if entry.unrelated {
            return Some(None);
        }
        entry.counts
    }

    pub fn put_counts(&mut self, base: &str, head: &str, counts: Option<Counts>) {
        let entry = self.entry(pair(base, head));
        if !entry.unrelated {
            entry.counts = Some(counts);
        }
    }

    pub fn lines(&mut self, base: &str, head: &str) -> Option<Option<Lines>> {
        let entry = self.touch(&pair(base, head))?;
        if entry.unrelated {
            return Some(None);
        }
        entry.lines.map(Some)
    }

    pub fn put_lines(&mut self, base: &str, head: &str, lines: Option<Lines>) {
        let entry = self.entry(pair(base, head));
        match lines {
            None => entry.unrelated = true,
            Some(found) if !entry.unrelated => {
                entry.lines = Some(found);
                entry.related = true;
            }
            Some(_) => {}
        }
    }

    pub fn related(&mut self, base: &str, head: &str) -> Option<bool> {
        let entry = self.touch(&pair(base, head))?;
        if entry.unrelated {
            Some(false)
        } else if entry.related {
            Some(true)
        } else {
            None
        }
    }

    pub fn put_related(&mut self, base: &str, head: &str, related: bool) {
        let entry = self.entry(pair(base, head));
        if !related {
            entry.unrelated = true;
        } else if !entry.unrelated {
            entry.related = true;
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Listed {
    pub worktrees: Vec<WorktreeEntry>,
    pub truncated: bool,
    pub remotes: Vec<(String, bool)>,
    pub default: Option<String>,
    pub fetched_at: Option<i64>,
}

#[derive(Default)]
pub struct RepoCache {
    pub listed: Option<(i64, Listed)>,
    pub numbers: Numbers,
    pub work: HashMap<String, Work>,
    pub facts: HashMap<String, (Option<i64>, String)>,
    pub reading: HashSet<String>,
    pub asked_ms: i64,
}

impl RepoCache {
    pub fn listed_fresh(&self, now_ms: i64) -> Option<&Listed> {
        let (at, listed) = self.listed.as_ref()?;
        (now_ms - at < LIST_FOR_MS).then_some(listed)
    }
}

#[derive(Default)]
pub struct GitCache {
    repos: HashMap<PathBuf, RepoCache>,
}

impl GitCache {
    pub fn repo(&mut self, common_dir: &Path, now_ms: i64) -> &mut RepoCache {
        self.repos
            .retain(|_, repo| now_ms - repo.asked_ms < IDLE_DROP_MS);
        let repo = self.repos.entry(common_dir.to_path_buf()).or_default();
        repo.asked_ms = now_ms;
        repo
    }

    #[cfg(test)]
    pub fn find_work(&self, path: &str) -> Option<Work> {
        self.repos
            .values()
            .find_map(|repo| repo.work.get(path).cloned())
    }
}

pub fn pick_reads(
    eligible: &[String],
    work: &HashMap<String, Work>,
    reading: &HashSet<String>,
    fresh: &[String],
    now_ms: i64,
) -> Vec<String> {
    let mut picked: Vec<String> = eligible
        .iter()
        .filter(|path| !work.contains_key(*path) && !reading.contains(*path))
        .take(MISSING_READS)
        .cloned()
        .collect();
    if picked.is_empty() {
        let stalest = eligible
            .iter()
            .filter(|path| !reading.contains(*path))
            .filter_map(|path| work.get(path).map(|found| (found.read_at, path)))
            .filter(|(read_at, _)| now_ms - read_at > STALE_MS)
            .min_by_key(|(read_at, _)| *read_at);
        if let Some((_, path)) = stalest {
            picked.push(path.clone());
        }
    }
    let mut added = 0;
    for path in fresh {
        if added >= FRESH_READS {
            break;
        }
        if reading.contains(path) || picked.contains(path) {
            continue;
        }
        picked.push(path.clone());
        added += 1;
    }
    picked
}

pub struct Budget {
    start_ms: i64,
    stop_ms: i64,
}

impl Budget {
    pub fn new(start_ms: i64, stop_ms: i64) -> Budget {
        Budget { start_ms, stop_ms }
    }

    pub fn may_start(&self, now_ms: i64) -> bool {
        now_ms - self.start_ms < self.stop_ms
    }
}
