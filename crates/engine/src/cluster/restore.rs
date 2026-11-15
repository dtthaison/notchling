use super::id::Tool;
use super::snapshot::{excluded_dir, Manifest, Movable, Part, PartKind, RepoFacts, SessionEntry};
use super::standby_store::{hash_file, hash_range, plain_len, StandbyStore};
use super::workspace::{origin_ok, valid_branch, GitCall, SLOW_SECS};
use crate::bridge::net::devices::FILE_NAME as DEVICES;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

pub const INDEX_FILE: &str = "session_index.jsonl";
pub const COPIES_DIR: &str = "copies";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathState {
    Missing,
    Clean { branch: String, head: String },
    Changed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreStep {
    CloneRepo {
        origin: String,
        path: PathBuf,
        branch: String,
        head: String,
    },
    FastForward {
        path: PathBuf,
        branch: String,
        head: String,
    },
    AddWorktree {
        main: PathBuf,
        path: PathBuf,
        branch: String,
        head: String,
    },
    AppendTail {
        from: PathBuf,
        to: PathBuf,
        offset: u64,
        end: u64,
        prefix_sha256: String,
    },
    PutFile {
        from: PathBuf,
        to: PathBuf,
        sha256: String,
        len: u64,
    },
    PutDir {
        from: PathBuf,
        to: PathBuf,
        sha256: String,
        len: u64,
    },
    MoveAside {
        path: PathBuf,
        to: PathBuf,
    },
    RestoreDb {
        from: PathBuf,
        to: PathBuf,
        sha256: String,
    },
    MergeDevices {
        from: PathBuf,
        to: PathBuf,
        sha256: String,
    },
    MergeIndex {
        from: PathBuf,
        to: PathBuf,
        session: String,
        sha256: String,
    },
}

pub struct RestoreCtx<'a> {
    pub cluster: &'a Path,
    pub home: &'a Path,
    pub node: &'a str,
    pub aside: bool,
    pub shared: bool,
}

pub fn copy_name(text: &str) -> String {
    let name: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if name.is_empty() {
        "_".to_owned()
    } else {
        name
    }
}

fn full_hash(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn call(cwd: &Path, args: &[&str]) -> GitCall {
    GitCall {
        cwd: cwd.to_string_lossy().into_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        skip_if_exists: None,
        allow_failure: false,
        refusals: Vec::new(),
        timeout_secs: SLOW_SECS,
    }
}

fn refusing(mut call: GitCall, when: &str, text: &str) -> GitCall {
    call.refusals.push((when.to_owned(), text.to_owned()));
    call
}

fn tracking(path: &Path, branch: &str) -> GitCall {
    let mut call = call(
        path,
        &[
            "branch",
            &format!("--set-upstream-to=origin/{branch}"),
            branch,
        ],
    );
    call.allow_failure = true;
    call
}

fn fetch(at: &Path, branch: &str) -> GitCall {
    refusing(
        call(
            at,
            &[
                "fetch",
                "--no-tags",
                "--",
                "origin",
                &format!("+refs/heads/{branch}:refs/remotes/origin/{branch}"),
            ],
        ),
        "",
        "Couldn't fetch the repository's remote on this machine",
    )
}

fn have(at: &Path, head: &str) -> GitCall {
    refusing(
        call(at, &["cat-file", "-e", &format!("{head}^{{commit}}")]),
        "",
        "The commit isn't on the remote",
    )
}

pub fn git_calls(step: &RestoreStep) -> Vec<GitCall> {
    match step {
        RestoreStep::CloneRepo {
            origin,
            path,
            branch,
            head,
        } => {
            let parent = path.parent().unwrap_or(path);
            let target = path.to_string_lossy();
            vec![
                refusing(
                    call(
                        parent,
                        &["clone", "--no-checkout", "--no-tags", "--", origin, &target],
                    ),
                    "",
                    "Couldn't clone the repository's remote on this machine",
                ),
                have(path, head),
                call(
                    path,
                    &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
                ),
                call(path, &["reset", "-q", "--hard", head]),
                tracking(path, branch),
            ]
        }
        RestoreStep::FastForward { path, branch, head } => vec![
            fetch(path, branch),
            have(path, head),
            refusing(
                call(path, &["merge-base", "--is-ancestor", "HEAD", head]),
                "",
                "The folder has commits that aren't in this set",
            ),
            call(path, &["merge", "-q", "--ff-only", head]),
        ],
        RestoreStep::AddWorktree {
            main,
            path,
            branch,
            head,
        } => {
            let target = path.to_string_lossy();
            vec![
                fetch(main, branch),
                have(main, head),
                refusing(
                    call(
                        main,
                        &["worktree", "add", "-b", branch, "--", &target, head],
                    ),
                    "already exists",
                    &format!("{branch} already exists in the main repository"),
                ),
                tracking(path, branch),
            ]
        }
        _ => Vec::new(),
    }
}

struct Planner<'a> {
    m: &'a Manifest,
    ctx: &'a RestoreCtx<'a>,
    store: StandbyStore,
    probe: &'a dyn Fn(&Path) -> PathState,
    refused: Vec<String>,
    repos: Vec<RestoreStep>,
    files: Vec<RestoreStep>,
    wanted: BTreeMap<PathBuf, (String, String)>,
    cloned: BTreeSet<PathBuf>,
    roots: Vec<PathBuf>,
}

impl<'a> Planner<'a> {
    fn node(&self) -> &str {
        self.ctx.node
    }

    fn place_ok(&self, path: &Path) -> Result<(), String> {
        let plain = path.is_absolute()
            && path
                .components()
                .all(|part| matches!(part, Component::RootDir | Component::Normal(_)));
        if !plain {
            return Err(format!("{} isn't a plain absolute path", path.display()));
        }
        if excluded_dir(path) {
            return Err(format!(
                "{} is in a folder a set never carries",
                path.display()
            ));
        }
        if !path.starts_with(self.ctx.home) {
            return Err(format!(
                "{} isn't under {}",
                path.display(),
                self.ctx.home.display()
            ));
        }
        Ok(())
    }

    fn file_ok(&self, path: &Path) -> Result<(), String> {
        self.place_ok(path)?;
        match self.roots.iter().find(|root| path.starts_with(root)) {
            Some(root) => Err(format!(
                "{} is inside the repository {}: code never travels",
                path.display(),
                root.display()
            )),
            None => Ok(()),
        }
    }

    fn refuse(&mut self, why: String) {
        if !self.refused.contains(&why) {
            self.refused.push(why);
        }
    }

    fn push_repo(&mut self, step: RestoreStep) {
        if !self.repos.contains(&step) {
            self.repos.push(step);
        }
    }

    fn push_file(&mut self, step: RestoreStep) {
        if !self.files.contains(&step) {
            self.files.push(step);
        }
    }

    fn session(&mut self, entry: &SessionEntry) {
        match &entry.movable {
            Movable::Yes => {}
            Movable::No(why) => {
                self.refuse(format!("{} can't move: {why}", entry.id));
                return;
            }
        }
        let Some(facts) = entry.repo.as_ref() else {
            self.refuse(format!("{} can't move: it has no repository", entry.id));
            return;
        };
        if let Err(why) = self.repo(entry, facts) {
            self.refuse(why);
            return;
        }
        for path in &entry.transcripts {
            if let Err(why) = self.transcript(entry, path) {
                self.refuse(why);
            }
        }
        for path in &entry.files {
            if let Err(why) = self.sidecar(entry, path) {
                self.refuse(why);
            }
        }
    }

    fn repo(&mut self, entry: &SessionEntry, facts: &RepoFacts) -> Result<(), String> {
        let id = &entry.id;
        let (Some(origin), Some(branch)) = (facts.origin.as_deref(), facts.branch.as_deref())
        else {
            return Err(format!("{id} can't move: it has no pushed branch"));
        };
        if !origin_ok(origin) {
            return Err(format!(
                "{id} can't move: its remote's address isn't one Notchling will clone from"
            ));
        }
        if !valid_branch(branch) || !full_hash(&facts.head) {
            return Err(format!("{id} can't move: its branch or commit isn't valid"));
        }
        let root = &facts.root;
        self.place_ok(root)?;
        let want = (branch.to_owned(), facts.head.clone());
        if let Some(seen) = self.wanted.get(root) {
            return if *seen == want {
                Ok(())
            } else {
                Err(format!(
                    "{} is needed at two commits by the sessions being restored",
                    root.display()
                ))
            };
        }
        self.wanted.insert(root.clone(), want);
        let (branch, head) = (branch.to_owned(), facts.head.clone());
        match (self.probe)(root) {
            PathState::Changed => Err(format!(
                "{} on {} has its own changes",
                root.display(),
                self.node()
            )),
            PathState::Clean {
                branch: there,
                head: at,
            } => {
                if there != branch {
                    return Err(format!(
                        "{} on {} is on {there}",
                        root.display(),
                        self.node()
                    ));
                }
                if at != head {
                    self.push_repo(RestoreStep::FastForward {
                        path: root.clone(),
                        branch,
                        head,
                    });
                }
                Ok(())
            }
            PathState::Missing => {
                let Some(main) = facts.main_repo.as_ref() else {
                    self.cloned.insert(root.clone());
                    self.push_repo(RestoreStep::CloneRepo {
                        origin: origin.to_owned(),
                        path: root.clone(),
                        branch,
                        head,
                    });
                    return Ok(());
                };
                self.place_ok(main)?;
                let there = self.cloned.contains(main) || (self.probe)(main) != PathState::Missing;
                if !there {
                    return Err(format!(
                        "{} isn't on {}: {} is a linked worktree of it",
                        main.display(),
                        self.node(),
                        root.display()
                    ));
                }
                self.push_repo(RestoreStep::AddWorktree {
                    main: main.clone(),
                    path: root.clone(),
                    branch,
                    head,
                });
                Ok(())
            }
        }
    }

    fn aside_to(&self, entry: &SessionEntry, path: &Path) -> PathBuf {
        let mut to = self
            .ctx
            .cluster
            .join(COPIES_DIR)
            .join(copy_name(&entry.id))
            .join(copy_name(&self.m.set));
        for part in path.components() {
            if let Component::Normal(name) = part {
                to.push(name);
            }
        }
        to
    }

    fn part(&self, path: &Path, want: impl Fn(&PartKind) -> bool) -> Option<&'a Part> {
        self.m
            .parts
            .iter()
            .find(|part| part.path == path && want(&part.kind))
    }

    fn transcript(&mut self, entry: &SessionEntry, path: &Path) -> Result<(), String> {
        self.file_ok(path)?;
        let id = &entry.id;
        let Some(part) = self.part(path, |kind| matches!(kind, PartKind::Tail { .. })) else {
            return Err(format!(
                "{id} can't move: its transcript {} isn't in the set",
                path.display()
            ));
        };
        let PartKind::Tail { from } = part.kind else {
            return Err(format!("{id} can't move: {} isn't a tail", path.display()));
        };
        let stored = self
            .store
            .stored_tail(path)
            .ok_or_else(|| format!("{id} can't move: {} can't be stored", path.display()))?;
        let end = from + part.len;
        let broken = || {
            format!(
                "{id} can't move: {} in the store doesn't match the set",
                path.display()
            )
        };
        if plain_len(&stored).map_err(|_| broken())? != Some(end)
            || hash_range(&stored, from, part.len).map_err(|_| broken())? != part.sha256
        {
            return Err(broken());
        }
        let local = plain_len(path)?;
        let whole = || hash_range(&stored, 0, end);
        let put = |sha256: String| RestoreStep::PutFile {
            from: stored.clone(),
            to: path.to_path_buf(),
            sha256,
            len: end,
        };
        let Some(have) = local.filter(|len| *len > 0) else {
            self.push_file(put(whole()?));
            return Ok(());
        };
        if have <= end {
            let prefix = hash_range(&stored, 0, have)?;
            if hash_range(path, 0, have)? == prefix {
                if have < end {
                    self.push_file(RestoreStep::AppendTail {
                        from: stored,
                        to: path.to_path_buf(),
                        offset: have,
                        end,
                        prefix_sha256: prefix,
                    });
                }
                return Ok(());
            }
        }
        if !self.ctx.aside {
            return Err(format!("{id} ran on {} after this set", self.node()));
        }
        let to = self.aside_to(entry, path);
        self.push_file(RestoreStep::MoveAside {
            path: path.to_path_buf(),
            to,
        });
        let sha256 = whole()?;
        self.push_file(put(sha256));
        Ok(())
    }

    fn sidecar(&mut self, entry: &SessionEntry, path: &Path) -> Result<(), String> {
        self.file_ok(path)?;
        let Some(part) = self.part(path, |kind| matches!(kind, PartKind::File | PartKind::Dir))
        else {
            return Ok(());
        };
        let from = self
            .store
            .stored(&self.m.set, path)
            .ok_or_else(|| format!("{} can't be stored", path.display()))?;
        if part.kind == PartKind::Dir {
            self.local_dir(path)?;
            self.push_file(RestoreStep::PutDir {
                from,
                to: path.to_path_buf(),
                sha256: part.sha256.clone(),
                len: part.len,
            });
            return Ok(());
        }
        if entry.tool == Tool::Codex && path.file_name().is_some_and(|name| name == INDEX_FILE) {
            let session = entry.id.strip_prefix("codex:").unwrap_or(&entry.id);
            self.push_file(RestoreStep::MergeIndex {
                from,
                to: path.to_path_buf(),
                session: session.to_owned(),
                sha256: part.sha256.clone(),
            });
            return Ok(());
        }
        match plain_len(path)? {
            None => {}
            Some(_) if hash_file(path)?.1 == part.sha256 => return Ok(()),
            Some(_) if self.ctx.aside => {
                let to = self.aside_to(entry, path);
                self.push_file(RestoreStep::MoveAside {
                    path: path.to_path_buf(),
                    to,
                });
            }
            Some(_) => {}
        }
        self.push_file(RestoreStep::PutFile {
            from,
            to: path.to_path_buf(),
            sha256: part.sha256.clone(),
            len: part.len,
        });
        Ok(())
    }

    fn local_dir(&self, path: &Path) -> Result<(), String> {
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() => Ok(()),
            Ok(_) => Err(format!(
                "{} isn't a folder on {}",
                path.display(),
                self.node()
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    fn shared(&mut self) {
        let owned: BTreeSet<&PathBuf> = self
            .m
            .sessions
            .iter()
            .flat_map(|entry| entry.transcripts.iter().chain(&entry.files))
            .collect();
        for part in &self.m.parts {
            if owned.contains(&part.path) {
                continue;
            }
            let name = part.path.file_name().is_some_and(|name| name == DEVICES);
            let step = match &part.kind {
                PartKind::Db => {
                    self.store
                        .stored(&self.m.set, &part.path)
                        .map(|from| RestoreStep::RestoreDb {
                            from,
                            to: part.path.clone(),
                            sha256: part.sha256.clone(),
                        })
                }
                PartKind::File if name => self.store.stored(&self.m.set, &part.path).map(|from| {
                    RestoreStep::MergeDevices {
                        from,
                        to: part.path.clone(),
                        sha256: part.sha256.clone(),
                    }
                }),
                PartKind::Dir => {
                    self.store
                        .stored(&self.m.set, &part.path)
                        .map(|from| RestoreStep::PutDir {
                            from,
                            to: part.path.clone(),
                            sha256: part.sha256.clone(),
                            len: part.len,
                        })
                }
                _ => None,
            };
            let Some(step) = step else { continue };
            match self.file_ok(&part.path).and_then(|()| match step.clone() {
                RestoreStep::PutDir { .. } => self.local_dir(&part.path),
                _ => Ok(()),
            }) {
                Ok(()) => self.push_file(step),
                Err(why) => self.refuse(why),
            }
        }
    }
}

pub fn plan_restore(
    m: &Manifest,
    ctx: &RestoreCtx,
    sessions: &BTreeSet<String>,
    probe: &dyn Fn(&Path) -> PathState,
) -> Result<Vec<RestoreStep>, String> {
    let mut roots = Vec::new();
    for entry in &m.sessions {
        if let Some(facts) = &entry.repo {
            roots.push(facts.root.clone());
            roots.extend(facts.main_repo.clone());
        }
    }
    let mut planner = Planner {
        m,
        ctx,
        store: StandbyStore::new(ctx.cluster, &m.home),
        probe,
        refused: Vec::new(),
        repos: Vec::new(),
        files: Vec::new(),
        wanted: BTreeMap::new(),
        cloned: BTreeSet::new(),
        roots,
    };
    let mut named: Vec<&SessionEntry> = Vec::new();
    for id in sessions {
        match m.sessions.iter().find(|entry| entry.id == *id) {
            Some(entry) => named.push(entry),
            None => planner.refuse(format!("{id} can't move: it isn't in {}", m.set)),
        }
    }
    named.sort_by_key(|entry| entry.repo.as_ref().is_some_and(|f| f.main_repo.is_some()));
    for entry in named {
        planner.session(entry);
    }
    if ctx.shared {
        planner.shared();
    }
    if !planner.refused.is_empty() {
        return Err(planner.refused.join("; "));
    }
    let mut steps = planner.repos;
    let (dbs, rest): (Vec<_>, Vec<_>) = planner
        .files
        .into_iter()
        .partition(|step| matches!(step, RestoreStep::RestoreDb { .. }));
    steps.extend(rest);
    steps.extend(dbs);
    Ok(steps)
}
