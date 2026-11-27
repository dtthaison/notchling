use crate::acting_ops::now_ms;
use crate::git_cache::{Budget, Kind, LINES_BUDGET_MS};
use crate::git_ops::{lock, GitOps};
use crate::git_view::Snapshot;
use crate::git_work_reads::await_reads;
use crate::git_worktrees::{absent, places, repo_name, Row};
use notchling_engine::git::branch_list::{rows, BranchRow, ListInput, Work};
use notchling_engine::git::nearest::Candidate;
use notchling_engine::git::reader::Reader;
use notchling_engine::git::refs::Lines;
use notchling_engine::git::worktrees::WorktreeEntry;
use notchling_local_api::ApiError;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;

type Pair = (String, String);

#[derive(Default)]
struct Gaps {
    counts: RefCell<Vec<(Candidate, String)>>,
    lines: RefCell<Vec<Pair>>,
}
type Places = Vec<(
    WorktreeEntry,
    notchling_engine::git::branch_list::WorktreeOf,
)>;

impl GitOps {
    pub(crate) fn branch_list(&self, cwd: &str) -> Result<Value, ApiError> {
        let start = now_ms();
        let Some((reader, key)) = self.open_repo(cwd)? else {
            return Ok(json!({"repo": false}));
        };
        let Some(snap) = absent(self.snapshot(&reader, &key, start))? else {
            return Ok(json!({"repo": false}));
        };
        let rows_in = self.rows_of(&snap, cwd);
        let readable: Vec<&Row> = rows_in
            .iter()
            .filter(|row| !row.entry.prunable && row.base.is_some())
            .collect();
        let jobs = self.pick_jobs(&key, &readable, &[], start);
        let reads = self.spawn_reads(&key, jobs, start);
        let lines_budget = Budget::new(now_ms(), LINES_BUDGET_MS);
        let bare = places(&rows_in, None);
        self.read_lines(&snap, &reader, &key, &bare, &lines_budget);
        await_reads(reads, start);
        let work: HashMap<String, Work> = lock(&self.cache).repo(&key, now_ms()).work.clone();
        let list = self.compute(&snap, &key, &places(&rows_in, Some(&work)), None);
        let pending = list.iter().filter(|row| row.pending).count();
        Ok(json!({
            "repo": true,
            "name": repo_name(&snap, &key),
            "default": snap.listed.default,
            "integration": snap.bases.integration.as_ref().map(|c| c.rev.clone()),
            "fetched_at": snap.listed.fetched_at,
            "truncated": snap.listed.truncated || snap.refs_truncated,
            "pending": pending,
            "branches": list,
        }))
    }

    fn compute(
        &self,
        snap: &Snapshot,
        key: &Path,
        places: &Places,
        gaps: Option<&Gaps>,
    ) -> Vec<BranchRow> {
        let input = ListInput {
            refs: &snap.refs,
            worktrees: places,
            remotes: &snap.listed.remotes,
            bases: &snap.bases,
            detached_facts: &snap.facts,
        };
        let counts = |base: &Candidate, head: &str| {
            let mut cache = lock(&self.cache);
            let repo = cache.repo(key, now_ms());
            let mut known = repo.numbers.counts(&base.commit, head);
            if known.is_none() && repo.failed.has(Kind::Counts, &base.commit, head, now_ms()) {
                known = Some(None);
            }
            drop(cache);
            if known.is_none() {
                if let Some(gaps) = gaps {
                    gaps.counts
                        .borrow_mut()
                        .push((base.clone(), head.to_owned()));
                }
            }
            known
        };
        let lines = |base: &Candidate, head: &str| {
            let mut cache = lock(&self.cache);
            let repo = cache.repo(key, now_ms());
            let mut known = repo.numbers.lines(&base.commit, head);
            if known.is_none() && repo.failed.has(Kind::Lines, &base.commit, head, now_ms()) {
                known = Some(None);
            }
            drop(cache);
            if known.is_none() {
                if let Some(gaps) = gaps {
                    gaps.lines
                        .borrow_mut()
                        .push((base.commit.clone(), head.to_owned()));
                }
            }
            known
        };
        rows(&input, &counts, &lines)
    }

    fn read_lines(
        &self,
        snap: &Snapshot,
        reader: &Reader,
        key: &Path,
        places: &Places,
        budget: &Budget,
    ) {
        let mut tried: HashSet<Pair> = HashSet::new();
        let mut counted: HashSet<Pair> = HashSet::new();
        loop {
            let gaps = Gaps::default();
            self.compute(snap, key, places, Some(&gaps));
            let mut unknown: Vec<(Candidate, String)> = Vec::new();
            for (base, head) in gaps.counts.into_inner() {
                let pair = (base.commit.clone(), head.clone());
                if counted.insert(pair) {
                    unknown.push((base, head));
                }
            }
            if !unknown.is_empty() {
                for (base, head) in unknown {
                    if !budget.may_start(now_ms()) {
                        return;
                    }
                    snap.count_pair(&base, &head);
                }
                continue;
            }
            let mut todo: Vec<Pair> = Vec::new();
            for pair in gaps.lines.into_inner() {
                if !tried.contains(&pair) && !todo.contains(&pair) {
                    todo.push(pair);
                }
            }
            if todo.is_empty() {
                return;
            }
            for pair in todo {
                if !budget.may_start(now_ms()) {
                    return;
                }
                tried.insert(pair.clone());
                let found: Result<Option<Lines>, _> = reader.lines(&pair.0, &pair.1);
                let mut cache = lock(&self.cache);
                let repo = cache.repo(key, now_ms());
                match found {
                    Ok(found) => repo.numbers.put_lines(&pair.0, &pair.1, found),
                    Err(_) => repo.failed.mark(Kind::Lines, &pair.0, &pair.1, now_ms()),
                }
            }
        }
    }
}
