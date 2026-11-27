use super::nearest::{candidates, pick, short_name, Candidate};
use super::refs::{plain_label, Counts, Lines, RefRow, SUBJECT_CHARS};
use super::worktrees::WorktreeEntry;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Work {
    pub added: u64,
    pub removed: u64,
    pub files: u64,
    pub uncommitted_files: u64,
    pub changed_at: Option<i64>,
    pub read_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorktreeOf {
    pub path: String,
    pub real: String,
    pub main: bool,
    pub current: bool,
    pub missing: bool,
    pub work: Option<Work>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct BranchRow {
    pub name: Option<String>,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub local: bool,
    pub remote: Option<String>,
    pub remote_only: bool,
    pub on_github: bool,
    pub is_base: bool,
    pub worktree: Option<WorktreeOf>,
    pub head: Option<String>,
    pub detached: bool,
    pub committed_at: Option<i64>,
    pub subject: Option<String>,
    pub upstream: Option<String>,
    pub upstream_ahead: u64,
    pub upstream_behind: u64,
    pub upstream_gone: bool,
    pub base: Option<String>,
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
    pub unrelated: bool,
    pub merged_into: Option<String>,
    pub lines: Option<Lines>,
    pub pending: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bases {
    pub default: Option<Candidate>,
    pub integration: Option<Candidate>,
}

pub struct ListInput<'a> {
    pub refs: &'a [RefRow],
    pub worktrees: &'a [(WorktreeEntry, WorktreeOf)],
    pub remotes: &'a [(String, bool)],
    pub bases: &'a Bases,
    pub detached_facts: &'a HashMap<String, (Option<i64>, String)>,
}

type CountsOf<'a> = &'a dyn Fn(&Candidate, &str) -> Option<Option<Counts>>;
type LinesOf<'a> = &'a dyn Fn(&Candidate, &str) -> Option<Option<Lines>>;

const HEADS: &str = "refs/heads/";
const REMOTES: &str = "refs/remotes/";

#[derive(Default)]
struct Placed {
    base: Option<String>,
    ahead: Option<u64>,
    behind: Option<u64>,
    unrelated: bool,
    merged_into: Option<String>,
    lines: Option<Lines>,
    pending: bool,
}

struct Subject<'a> {
    own: Option<&'a str>,
    upstream: Option<Candidate>,
    head: &'a str,
}

struct Rules<'a> {
    input: &'a ListInput<'a>,
    names: Vec<String>,
    counts: CountsOf<'a>,
    lines: LinesOf<'a>,
}

fn split_remote<'a>(rest: &'a str, names: &[String]) -> Option<(&'a str, &'a str)> {
    let name = short_name(rest, names);
    let split = if name.len() < rest.len() {
        Some((&rest[..rest.len() - name.len() - 1], name))
    } else {
        rest.split_once('/')
    };
    split.filter(|(remote, name)| !remote.is_empty() && !name.is_empty())
}

fn short_head(commit: &str) -> String {
    commit.chars().take(7).collect()
}

impl Rules<'_> {
    fn is_base(&self, rev: &str, local: bool) -> bool {
        let bases = self.input.bases;
        let matches = |base: &Option<Candidate>| {
            base.as_ref().is_some_and(|base| {
                if local {
                    short_name(&base.rev, &self.names) == rev
                } else {
                    base.rev == rev
                }
            })
        };
        matches(&bases.default) || matches(&bases.integration)
    }

    fn place(&self, subject: &Subject) -> Placed {
        let bases = self.input.bases;
        let list = candidates(
            subject.own,
            subject.upstream.as_ref(),
            bases.default.as_ref(),
            bases.integration.as_ref(),
            &self.names,
        );
        let mut placed = Placed::default();
        let mut scored: Vec<(Candidate, Option<Counts>)> = Vec::new();
        for candidate in &list {
            match (self.counts)(candidate, subject.head) {
                None => placed.pending = true,
                Some(counts) => scored.push((candidate.clone(), counts)),
            }
        }
        if placed.pending {
            return placed;
        }
        for base in [&bases.integration, &bases.default].into_iter().flatten() {
            let ahead_zero = scored
                .iter()
                .any(|(c, n)| c.rev == base.rev && n.is_some_and(|n| n.ahead == 0));
            if ahead_zero {
                placed.merged_into = Some(base.rev.clone());
                break;
            }
        }
        loop {
            let Some((candidate, counts)) = pick(&scored) else {
                placed.unrelated = !list.is_empty();
                return placed;
            };
            if placed.merged_into.is_some() || counts.ahead == 0 {
                placed.lines = placed.merged_into.is_none().then(Lines::default);
                return self.settle(placed, candidate, counts);
            }
            match (self.lines)(&candidate, subject.head) {
                None => {
                    placed.pending = true;
                    return self.settle(placed, candidate, counts);
                }
                Some(Some(lines)) => {
                    placed.lines = Some(lines);
                    return self.settle(placed, candidate, counts);
                }
                Some(None) => {
                    for entry in scored.iter_mut().filter(|(c, _)| c.rev == candidate.rev) {
                        entry.1 = None;
                    }
                }
            }
        }
    }

    fn settle(&self, mut placed: Placed, candidate: Candidate, counts: Counts) -> Placed {
        placed.base = Some(candidate.rev);
        placed.ahead = Some(counts.ahead);
        placed.behind = Some(counts.behind);
        placed
    }

    fn row(&self, mut row: BranchRow, subject: &Subject) -> BranchRow {
        if !row.is_base {
            let placed = self.place(subject);
            row.base = placed.base;
            row.ahead = placed.ahead;
            row.behind = placed.behind;
            row.unrelated = placed.unrelated;
            row.merged_into = placed.merged_into;
            row.lines = placed.lines;
            row.pending = placed.pending;
        }
        row
    }
}

fn ref_row(entry: &RefRow, name: &str, remote: Option<&str>) -> BranchRow {
    BranchRow {
        name: Some(name.to_string()),
        reference: Some(entry.refname.clone()),
        local: remote.is_none(),
        remote: remote.map(str::to_string),
        remote_only: remote.is_some(),
        head: Some(short_head(&entry.commit)),
        committed_at: entry.committed_ms,
        subject: Some(entry.subject.clone()).filter(|text| !text.is_empty()),
        upstream: entry.upstream.clone(),
        upstream_ahead: entry.track.ahead,
        upstream_behind: entry.track.behind,
        upstream_gone: entry.track.gone,
        ..BranchRow::default()
    }
}

pub fn rows(input: &ListInput, counts: CountsOf, lines: LinesOf) -> Vec<BranchRow> {
    let rules = Rules {
        input,
        names: input.remotes.iter().map(|(name, _)| name.clone()).collect(),
        counts,
        lines,
    };
    let mut commits: HashMap<&str, &str> = HashMap::new();
    for entry in input.refs {
        if let Some(name) = entry.refname.strip_prefix(HEADS) {
            commits.insert(name, &entry.commit);
        }
    }
    for entry in input.refs {
        if let Some(name) = entry.refname.strip_prefix(REMOTES) {
            commits.insert(name, &entry.commit);
        }
    }
    let local: HashSet<&str> = input
        .refs
        .iter()
        .filter_map(|entry| entry.refname.strip_prefix(HEADS))
        .collect();
    let github = |remote: &str| {
        input
            .remotes
            .iter()
            .any(|(name, on_github)| name == remote && *on_github)
    };
    let tree_of = |name: &str| {
        input
            .worktrees
            .iter()
            .find(|(entry, _)| !entry.bare && entry.branch.as_deref() == Some(name))
            .map(|(_, place)| place.clone())
    };
    let upstream_of = |entry: &RefRow| {
        let rev = entry.upstream.as_deref()?;
        let commit = commits.get(rev)?;
        Some(Candidate {
            rev: rev.to_string(),
            commit: commit.to_string(),
        })
    };

    let mut out = Vec::new();
    for entry in input.refs {
        let Some(name) = entry.refname.strip_prefix(HEADS) else {
            continue;
        };
        let mut row = ref_row(entry, name, None);
        row.is_base = rules.is_base(name, true);
        row.worktree = tree_of(name);
        let subject = Subject {
            own: Some(name),
            upstream: upstream_of(entry),
            head: &entry.commit,
        };
        out.push(rules.row(row, &subject));
    }
    for entry in input.refs {
        let Some(rest) = entry.refname.strip_prefix(REMOTES) else {
            continue;
        };
        let Some((remote, name)) = split_remote(rest, &rules.names) else {
            continue;
        };
        if name == "HEAD" || local.contains(name) {
            continue;
        }
        let mut row = ref_row(entry, name, Some(remote));
        row.on_github = github(remote);
        row.is_base = rules.is_base(rest, false);
        let subject = Subject {
            own: Some(name),
            upstream: None,
            head: &entry.commit,
        };
        out.push(rules.row(row, &subject));
    }
    for (entry, place) in input.worktrees {
        if entry.bare || !entry.detached {
            continue;
        }
        let facts = entry
            .head
            .as_ref()
            .and_then(|head| input.detached_facts.get(head));
        let row = BranchRow {
            detached: true,
            head: entry.head.as_deref().map(short_head),
            committed_at: facts.and_then(|(time, _)| *time),
            subject: facts
                .map(|(_, text)| plain_label(text, SUBJECT_CHARS))
                .filter(|text| !text.is_empty()),
            worktree: Some(place.clone()),
            ..BranchRow::default()
        };
        match &entry.head {
            Some(head) => {
                let subject = Subject {
                    own: None,
                    upstream: None,
                    head,
                };
                out.push(rules.row(row, &subject));
            }
            None => out.push(row),
        }
    }
    out
}

pub fn no_worktree(rows: &[BranchRow]) -> u64 {
    rows.iter().filter(|row| row.worktree.is_none()).count() as u64
}
