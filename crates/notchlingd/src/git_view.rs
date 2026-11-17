use crate::acting_ops::now_ms;
use crate::git_cache::{Budget, Kind, Listed, READ_STOP_MS};
use crate::git_ops::{lock, GitOps};
use notchling_engine::git::branch_list::Bases;
use notchling_engine::git::nearest::{candidates, pick, short_name, Candidate};
use notchling_engine::git::reader::Reader;
use notchling_engine::git::refs::{Counts, RefRow};
use notchling_engine::git::Failure;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub enum Placed {
    Picked(Candidate, Counts),
    Unrelated,
    Pending,
    Nothing,
}

pub struct Snapshot<'a> {
    pub listed: Listed,
    pub refs: Vec<RefRow>,
    pub refs_truncated: bool,
    pub bases: Bases,
    pub counts: HashMap<(String, String), Option<Counts>>,
    pub facts: HashMap<String, (Option<i64>, String)>,
    ops: &'a GitOps,
    reader: &'a Reader<'a>,
    key: PathBuf,
    budget: Budget,
}

fn folder_gone(path: &str) -> bool {
    matches!(
        std::fs::symlink_metadata(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    )
}

fn commit_in(refs: &[RefRow], rev: &str) -> Option<String> {
    let heads = format!("refs/heads/{rev}");
    let remotes = format!("refs/remotes/{rev}");
    [heads, remotes].iter().find_map(|name| {
        refs.iter()
            .find(|row| row.refname == *name)
            .map(|row| row.commit.clone())
    })
}

fn candidate_of(refs: &[RefRow], rev: &str) -> Option<Candidate> {
    commit_in(refs, rev).map(|commit| Candidate {
        rev: rev.to_owned(),
        commit,
    })
}

fn bases_of(listed: &Listed, refs: &[RefRow]) -> Bases {
    let names: Vec<String> = listed.remotes.iter().map(|(n, _)| n.clone()).collect();
    let default = listed
        .default
        .as_deref()
        .and_then(|rev| candidate_of(refs, rev));
    let default_short = listed
        .default
        .as_deref()
        .map(|rev| short_name(rev, &names).to_owned());
    let main = listed.worktrees.first().filter(|entry| !entry.bare);
    let integration = main
        .filter(|entry| !entry.detached)
        .and_then(|entry| entry.branch.as_deref())
        .filter(|branch| default_short.as_deref() != Some(*branch))
        .and_then(|branch| candidate_of(refs, branch));
    Bases {
        default,
        integration,
    }
}

impl GitOps {
    pub fn snapshot<'a>(
        &'a self,
        reader: &'a Reader<'a>,
        cache_key: &Path,
        start_ms: i64,
    ) -> Result<Snapshot<'a>, Failure> {
        let mut snap = self.view(reader, cache_key, start_ms)?;
        snap.fill_counts();
        snap.fill_facts();
        Ok(snap)
    }

    pub fn view<'a>(
        &'a self,
        reader: &'a Reader<'a>,
        cache_key: &Path,
        start_ms: i64,
    ) -> Result<Snapshot<'a>, Failure> {
        let (refs, refs_truncated) = reader.refs()?;
        let cached = lock(&self.cache)
            .repo(cache_key, start_ms)
            .listed_fresh(start_ms)
            .cloned();
        let listed = match cached {
            Some(listed) => listed,
            None => {
                let (worktrees, truncated) = reader.worktrees()?;
                let remotes = reader.remotes()?;
                let local: Vec<String> = refs
                    .iter()
                    .filter_map(|row| row.refname.strip_prefix("refs/heads/"))
                    .map(str::to_owned)
                    .collect();
                let listed = Listed {
                    worktrees,
                    truncated,
                    remotes,
                    default: reader.default_rev(&local),
                    fetched_at: Reader::fetched_at(cache_key),
                };
                lock(&self.cache).repo(cache_key, start_ms).listed =
                    Some((start_ms, listed.clone()));
                listed
            }
        };
        let mut listed = listed;
        for entry in listed.worktrees.iter_mut() {
            if !entry.bare && !entry.prunable && folder_gone(&entry.path) {
                entry.prunable = true;
            }
        }
        let bases = bases_of(&listed, &refs);
        Ok(Snapshot {
            listed,
            refs,
            refs_truncated,
            bases,
            counts: HashMap::new(),
            facts: HashMap::new(),
            ops: self,
            reader,
            key: cache_key.to_path_buf(),
            budget: Budget::new(start_ms, READ_STOP_MS),
        })
    }
}

impl Snapshot<'_> {
    fn may_start(&self) -> bool {
        self.budget.may_start(now_ms())
    }

    pub fn names(&self) -> Vec<String> {
        self.listed
            .remotes
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub fn commit_of(&self, rev: &str) -> Option<String> {
        commit_in(&self.refs, rev)
    }

    fn detached_heads(&self) -> Vec<String> {
        self.listed
            .worktrees
            .iter()
            .filter(|entry| entry.detached && !entry.bare && !entry.prunable)
            .filter_map(|entry| entry.head.clone())
            .collect()
    }

    fn lookup(&self, base: &str, head: &str) -> Option<Option<Counts>> {
        lock(&self.ops.cache)
            .repo(&self.key, now_ms())
            .numbers
            .counts(base, head)
    }

    fn failed(&self, base: &str, head: &str) -> bool {
        lock(&self.ops.cache).repo(&self.key, now_ms()).failed.has(
            Kind::Counts,
            base,
            head,
            now_ms(),
        )
    }

    fn fail(&self, base: &str, head: &str) {
        lock(&self.ops.cache).repo(&self.key, now_ms()).failed.mark(
            Kind::Counts,
            base,
            head,
            now_ms(),
        );
    }

    fn store(&self, base: &str, head: &str, counts: Counts) {
        lock(&self.ops.cache)
            .repo(&self.key, now_ms())
            .numbers
            .put_counts(base, head, Some(counts));
    }

    fn fill_counts(&mut self) {
        let mut heads: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for commit in self
            .refs
            .iter()
            .map(|row| row.commit.clone())
            .chain(self.detached_heads())
        {
            if seen.insert(commit.clone()) {
                heads.push(commit);
            }
        }
        let bases: Vec<Candidate> = [&self.bases.default, &self.bases.integration]
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        let mut old_git = false;
        for base in &bases {
            let missing = |snap: &Snapshot| -> Vec<String> {
                heads
                    .iter()
                    .filter(|head| snap.lookup(&base.commit, head).is_none())
                    .filter(|head| !snap.failed(&base.commit, head))
                    .cloned()
                    .collect()
            };
            if missing(self).is_empty() {
                continue;
            }
            if !old_git && self.may_start() {
                match self.reader.ahead_behind_all(&base.commit) {
                    Ok(Some(found)) => {
                        for row in &self.refs {
                            if let Some((commit, counts)) = found.get(&row.refname) {
                                self.store(&base.commit, commit, *counts);
                            }
                        }
                    }
                    Ok(None) => old_git = true,
                    Err(_) => {}
                }
            }
            for head in missing(self) {
                if !self.may_start() {
                    break;
                }
                match self.reader.left_right(&base.commit, &head) {
                    Ok(Some(counts)) => self.store(&base.commit, &head, counts),
                    _ => self.fail(&base.commit, &head),
                }
            }
        }
        let mut counts = HashMap::new();
        for base in &bases {
            for head in &heads {
                if let Some(found) = self.lookup(&base.commit, head) {
                    counts.insert((base.commit.clone(), head.clone()), found);
                }
            }
        }
        self.counts = counts;
    }

    fn fill_facts(&mut self) {
        for head in self.detached_heads() {
            let known = lock(&self.ops.cache)
                .repo(&self.key, now_ms())
                .facts
                .get(&head)
                .cloned();
            let facts = match known {
                Some(facts) => Some(facts),
                None if self.may_start() => match self.reader.commit_facts(&head) {
                    Ok(facts) => {
                        lock(&self.ops.cache)
                            .repo(&self.key, now_ms())
                            .facts
                            .insert(head.clone(), facts.clone());
                        Some(facts)
                    }
                    Err(_) => None,
                },
                None => None,
            };
            if let Some(facts) = facts {
                self.facts.insert(head, facts);
            }
        }
    }

    pub fn count_pair(&self, base: &Candidate, head: &str) -> Option<Option<Counts>> {
        if let Some(known) = self.lookup(&base.commit, head) {
            return Some(known);
        }
        if self.failed(&base.commit, head) {
            return Some(None);
        }
        if !self.may_start() {
            return None;
        }
        match self.reader.left_right(&base.commit, head) {
            Ok(Some(found)) => {
                self.store(&base.commit, head, found);
                self.lookup(&base.commit, head)
            }
            _ => {
                self.fail(&base.commit, head);
                Some(None)
            }
        }
    }

    fn confirm(&self, base: &str, head: &str) -> Option<bool> {
        let known = lock(&self.ops.cache)
            .repo(&self.key, now_ms())
            .numbers
            .related(base, head);
        if known.is_some() {
            return known;
        }
        if !self.may_start() {
            return None;
        }
        let related = self.reader.related(base, head).ok()?;
        lock(&self.ops.cache)
            .repo(&self.key, now_ms())
            .numbers
            .put_related(base, head, related);
        Some(related)
    }

    pub fn place(&self, head: &str, own: Option<&str>, upstream: Option<&str>) -> Placed {
        let names = self.names();
        let upstream = upstream.and_then(|rev| candidate_of(&self.refs, rev));
        let list = candidates(
            own,
            upstream.as_ref(),
            self.bases.default.as_ref(),
            self.bases.integration.as_ref(),
            &names,
        );
        if list.is_empty() {
            return Placed::Nothing;
        }
        let mut scored: Vec<(Candidate, Option<Counts>)> = Vec::new();
        for candidate in &list {
            match self.count_pair(candidate, head) {
                None => return Placed::Pending,
                Some(counts) => scored.push((candidate.clone(), counts)),
            }
        }
        loop {
            let Some((candidate, counts)) = pick(&scored) else {
                return Placed::Unrelated;
            };
            if counts.ahead == 0 {
                return Placed::Picked(candidate, counts);
            }
            match self.confirm(&candidate.commit, head) {
                Some(true) => return Placed::Picked(candidate, counts),
                Some(false) => {
                    for entry in scored.iter_mut().filter(|(c, _)| c.rev == candidate.rev) {
                        entry.1 = None;
                    }
                }
                None => return Placed::Pending,
            }
        }
    }
}
