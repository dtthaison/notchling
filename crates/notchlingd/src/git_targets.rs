use crate::acting_ops::now_ms;
use crate::git_args::{ref_arg, worktree_arg};
use crate::git_ops::{failed, lock, optional_arg, text_arg, GitOps};
use crate::git_view::{Placed, Snapshot};
use crate::git_worktrees::{absent, prefix_len};
use notchling_engine::git::branch_list::Work;
use notchling_engine::git::nearest::short_name;
use notchling_engine::git::reader::Reader;
use notchling_engine::git::refs::RefRow;
use notchling_engine::git::worktrees::WorktreeEntry;
use notchling_engine::git::{
    read_branch_diff, read_branch_stat, read_branches, read_ref_diff, read_ref_stat, BranchStat,
    Failure,
};
use notchling_local_api::ApiError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const HEADS: &str = "refs/heads/";
const REMOTES: &str = "refs/remotes/";
const NEAREST: &str = "nearest";

struct Asked<'a> {
    worktree: Option<&'a str>,
    rev: Option<&'a str>,
    nearest: bool,
}

fn asked(args: &Value) -> Result<Asked<'_>, ApiError> {
    let worktree = optional_arg(args, "worktree");
    let rev = optional_arg(args, "ref");
    if worktree.is_some() && rev.is_some() {
        return Err(ApiError::bad_request(
            "worktree and ref can't be asked together",
        ));
    }
    let nearest = match args.get("pick") {
        None | Some(Value::Null) => false,
        Some(Value::String(pick)) if pick == NEAREST => true,
        Some(_) => return Err(ApiError::bad_request("pick must be nearest")),
    };
    Ok(Asked {
        worktree,
        rev,
        nearest,
    })
}

fn settle(result: Result<BranchStat, Failure>) -> Result<Option<BranchStat>, ApiError> {
    match result {
        Ok(stat) => Ok(Some(stat)),
        Err(Failure::NotRepo) => Ok(None),
        Err(other) => Err(failed(other)),
    }
}

fn stat_reply(stat: &BranchStat, uncommitted: bool) -> Result<Value, ApiError> {
    let mut value =
        serde_json::to_value(stat).map_err(|e| ApiError::internal(format!("branch stat: {e}")))?;
    value["repo"] = json!(true);
    value["includes_uncommitted"] = json!(uncommitted);
    Ok(value)
}

fn refs_only(reader: &Reader) -> Result<Option<Vec<RefRow>>, ApiError> {
    match reader.refs() {
        Ok((rows, _)) => Ok(Some(rows)),
        Err(Failure::NotRepo) => Ok(None),
        Err(other) => Err(failed(other)),
    }
}

fn entry_at<'a>(snap: &'a Snapshot, at: &Path) -> Option<(&'a WorktreeEntry, String)> {
    snap.listed
        .worktrees
        .iter()
        .filter(|entry| !entry.bare && !entry.prunable)
        .filter_map(|entry| {
            let real = std::fs::canonicalize(&entry.path)
                .ok()?
                .to_string_lossy()
                .into_owned();
            prefix_len(at, entry, &real).map(|len| (len, entry, real))
        })
        .max_by_key(|(len, _, _)| *len)
        .map(|(_, entry, real)| (entry, real))
}

fn head_of(snap: &Snapshot, entry: &WorktreeEntry) -> Option<(String, Option<String>)> {
    let row = entry.branch.as_ref().and_then(|branch| {
        let name = format!("{HEADS}{branch}");
        snap.refs.iter().find(|row| row.refname == name)
    });
    let commit = row
        .map(|row| row.commit.clone())
        .or_else(|| entry.head.clone())?;
    Some((commit, row.and_then(|row| row.upstream.clone())))
}

enum Pick {
    Rule,
    Base(String),
    NoBase,
}

impl Pick {
    fn of(placed: Placed) -> Pick {
        match placed {
            Placed::Picked(candidate, _) => Pick::Base(candidate.rev),
            Placed::Unrelated => Pick::NoBase,
            Placed::Pending | Placed::Nothing => Pick::Rule,
        }
    }

    fn base(self) -> Option<String> {
        match self {
            Pick::Base(rev) => Some(rev),
            Pick::Rule | Pick::NoBase => None,
        }
    }
}

fn nearest_of_tree(snap: &Snapshot, entry: &WorktreeEntry) -> Pick {
    let Some((commit, upstream)) = head_of(snap, entry) else {
        return Pick::Rule;
    };
    Pick::of(snap.place(&commit, entry.branch.as_deref(), upstream.as_deref()))
}

fn nearest_of_ref(snap: &Snapshot, name: &str) -> Option<String> {
    let row = snap.refs.iter().find(|row| row.refname == name)?;
    let (own, upstream) = match name.strip_prefix(HEADS) {
        Some(local) => (local.to_owned(), row.upstream.clone()),
        None => {
            let rest = name.strip_prefix(REMOTES).unwrap_or(name);
            (short_name(rest, &snap.names()).to_owned(), None)
        }
    };
    Pick::of(snap.place(&row.commit, Some(&own), upstream.as_deref())).base()
}

fn unbased(snap: &Snapshot, entry: &WorktreeEntry) -> BranchStat {
    let head = head_of(snap, entry).map(|(commit, _)| commit.chars().take(7).collect());
    BranchStat {
        branch: entry.branch.clone().filter(|_| !entry.detached),
        head,
        detached: entry.detached,
        ..BranchStat::default()
    }
}

impl GitOps {
    pub(crate) fn branch_diff_stat(&self, cwd: &str, args: &Value) -> Result<Value, ApiError> {
        let want = asked(args)?;
        let base = optional_arg(args, "base");
        if want.worktree.is_none() && want.rev.is_none() && !want.nearest {
            return match settle(read_branch_stat(cwd, base, self.launcher.as_ref()))? {
                Some(stat) => stat_reply(&stat, true),
                None => Ok(json!({"repo": false})),
            };
        }
        let start = now_ms();
        let Some((reader, key)) = self.open_repo(cwd)? else {
            return Ok(json!({"repo": false}));
        };
        match want.rev {
            Some(raw) => self.ref_stat(&reader, &key, cwd, raw, base, want.nearest, start),
            None => self.tree_stat(&reader, &key, cwd, want.worktree, base, want.nearest, start),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn tree_stat(
        &self,
        reader: &Reader,
        key: &Path,
        cwd: &str,
        worktree: Option<&str>,
        base: Option<&str>,
        nearest: bool,
        start: i64,
    ) -> Result<Value, ApiError> {
        let Some(snap) = absent(self.view(reader, key, start))? else {
            return Ok(json!({"repo": false}));
        };
        let at: PathBuf = match worktree {
            Some(raw) => worktree_arg(raw, &snap.listed.worktrees)?,
            None => PathBuf::from(cwd),
        };
        let placed = entry_at(&snap, &at);
        let picked = match (&placed, nearest && base.is_none()) {
            (Some((entry, _)), true) => nearest_of_tree(&snap, entry),
            _ => Pick::Rule,
        };
        if let (Pick::NoBase, Some((entry, _))) = (&picked, &placed) {
            return stat_reply(&unbased(&snap, entry), true);
        }
        let run_in = match worktree {
            Some(_) => at.to_string_lossy().into_owned(),
            None => cwd.to_owned(),
        };
        let picked = picked.base();
        let chosen = base.map(str::to_owned).or_else(|| picked.clone());
        let result = read_branch_stat(&run_in, chosen.as_deref(), self.launcher.as_ref());
        let Some(stat) = settle(result)? else {
            return Ok(json!({"repo": false}));
        };
        if let (Some(_), Some((_, real)), Some(_)) = (&picked, &placed, &stat.base) {
            self.store_work(key, real, &stat);
        }
        stat_reply(&stat, true)
    }

    fn store_work(&self, key: &Path, real: &str, stat: &BranchStat) {
        let work = Work {
            added: stat.added,
            removed: stat.removed,
            files: stat.file_count,
            uncommitted_files: stat.uncommitted_files,
            changed_at: stat.changed_at,
            read_at: now_ms(),
        };
        lock(&self.cache)
            .repo(key, now_ms())
            .work
            .insert(real.to_owned(), work);
    }

    #[allow(clippy::too_many_arguments)]
    fn ref_stat(
        &self,
        reader: &Reader,
        key: &Path,
        cwd: &str,
        raw: &str,
        base: Option<&str>,
        nearest: bool,
        start: i64,
    ) -> Result<Value, ApiError> {
        let (name, picked) = if nearest && base.is_none() {
            let Some(snap) = absent(self.view(reader, key, start))? else {
                return Ok(json!({"repo": false}));
            };
            let name = ref_arg(raw, &snap.refs)?.to_owned();
            let picked = nearest_of_ref(&snap, &name);
            (name, picked)
        } else {
            let Some(refs) = refs_only(reader)? else {
                return Ok(json!({"repo": false}));
            };
            (ref_arg(raw, &refs)?.to_owned(), None)
        };
        let chosen = base.map(str::to_owned).or(picked);
        let result = read_ref_stat(cwd, chosen.as_deref(), &name, self.launcher.as_ref());
        match settle(result)? {
            Some(stat) => stat_reply(&stat, false),
            None => Ok(json!({"repo": false})),
        }
    }

    fn worktree_in(&self, cwd: &str, raw: &str) -> Result<Option<PathBuf>, ApiError> {
        let start = now_ms();
        let Some((reader, key)) = self.open_repo(cwd)? else {
            return Ok(None);
        };
        let Some(snap) = absent(self.view(&reader, &key, start))? else {
            return Ok(None);
        };
        worktree_arg(raw, &snap.listed.worktrees).map(Some)
    }

    pub(crate) fn branch_diff_file(&self, cwd: &str, args: &Value) -> Result<Value, ApiError> {
        let base = text_arg(args, "base")?;
        let path = text_arg(args, "path")?;
        let from = optional_arg(args, "from");
        let want = asked(args)?;
        let launcher = self.launcher.as_ref();
        let diff = match (want.worktree, want.rev) {
            (Some(raw), _) => {
                let Some(real) = self.worktree_in(cwd, raw)? else {
                    return Err(failed(Failure::NotRepo));
                };
                read_branch_diff(&real.to_string_lossy(), base, path, from, launcher)
            }
            (None, Some(raw)) => {
                let Some((reader, _)) = self.open_repo(cwd)? else {
                    return Err(failed(Failure::NotRepo));
                };
                let Some(refs) = refs_only(&reader)? else {
                    return Err(failed(Failure::NotRepo));
                };
                let name = ref_arg(raw, &refs)?;
                read_ref_diff(cwd, base, name, path, from, launcher)
            }
            (None, None) => read_branch_diff(cwd, base, path, from, launcher),
        }
        .map_err(failed)?;
        serde_json::to_value(diff).map_err(|e| ApiError::internal(format!("diff: {e}")))
    }

    pub(crate) fn branches(&self, cwd: &str, args: &Value) -> Result<Value, ApiError> {
        let at = match optional_arg(args, "worktree") {
            Some(raw) => match self.worktree_in(cwd, raw)? {
                Some(real) => real.to_string_lossy().into_owned(),
                None => return Ok(json!({"repo": false})),
            },
            None => cwd.to_owned(),
        };
        match read_branches(&at, self.launcher.as_ref()) {
            Ok(list) => {
                let mut value = serde_json::to_value(list)
                    .map_err(|e| ApiError::internal(format!("branches: {e}")))?;
                value["repo"] = json!(true);
                Ok(value)
            }
            Err(Failure::NotRepo) => Ok(json!({"repo": false})),
            Err(other) => Err(failed(other)),
        }
    }

    #[cfg(test)]
    pub(crate) fn cached_work(&self, path: &Path) -> Option<Work> {
        lock(&self.cache).find_work(&path.to_string_lossy())
    }
}
