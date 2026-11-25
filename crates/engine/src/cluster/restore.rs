use super::id::Tool;
use super::persist::CLUSTER_DB;
use super::snapshot::plan::bridge_devices;
use super::snapshot::{
    excluded_dir, shaped, Manifest, Movable, Part, PartKind, RepoFacts, SessionEntry, Shape,
};
use super::standby_store::{hash_file, hash_range, plain_len, StandbyStore};
use super::workspace::{origin_ok, valid_branch, GitCall, HARDENING, QUICK_SECS, SLOW_SECS};
use crate::git::read::driver_names;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

pub const INDEX_FILE: &str = "session_index.jsonl";
pub const COPIES_DIR: &str = "copies";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathState {
    Missing,
    Clean { branch: String, head: String },
    Changed,
    Foreign,
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
        origin: String,
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
    pub path_dirs: &'a [PathBuf],
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

pub fn folded_starts_with(path: &Path, base: &Path) -> bool {
    let mut path = path.components();
    base.components().all(|part| {
        path.next().is_some_and(|other| {
            same(
                &part.as_os_str().to_string_lossy(),
                &other.as_os_str().to_string_lossy(),
            )
        })
    })
}

fn folded_eq(a: &Path, b: &Path) -> bool {
    folded_starts_with(a, b) && folded_starts_with(b, a)
}

pub fn origin_key(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let trimmed = trimmed.trim_end_matches('/');
    if let Some((scheme, rest)) = trimmed.split_once("://") {
        if scheme == "file" {
            return rest.to_owned();
        }
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        let (name, port) = match host.rsplit_once(':') {
            Some((name, port)) if !host.ends_with(']') => (name, Some(port)),
            _ => (host, None),
        };
        let default = matches!(
            (scheme, port),
            ("ssh", Some("22")) | ("https", Some("443")) | ("git", Some("9418"))
        );
        let name = name.to_ascii_lowercase();
        return match port {
            Some(port) if !default => format!("{name}:{port}/{path}"),
            _ => format!("{name}/{path}"),
        };
    }
    if trimmed.starts_with('/') {
        return trimmed.to_owned();
    }
    match trimmed.split_once(':') {
        Some((host, path)) => {
            let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
            format!(
                "{}/{}",
                host.to_ascii_lowercase(),
                path.trim_start_matches('/')
            )
        }
        None => trimmed.to_owned(),
    }
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

pub fn drivers_probe(cwd: &Path) -> GitCall {
    let mut probe = call(cwd, &["config", "-z", "--get-regexp", "^filter\\."]);
    probe.allow_failure = true;
    probe.timeout_secs = QUICK_SECS;
    probe
}

pub fn blank_filters(call: &GitCall, probe_stdout: &str) -> Result<Vec<String>, String> {
    let names = driver_names(probe_stdout);
    if names.iter().any(|n| n.contains('=') || n.contains('\n')) {
        return Err("a filter name git can't be told to skip".to_owned());
    }
    let mut argv: Vec<String> = HARDENING.iter().map(|a| (*a).to_owned()).collect();
    for name in &names {
        for key in ["clean", "smudge", "process"] {
            argv.push("-c".to_owned());
            argv.push(format!("filter.{name}.{key}="));
        }
        argv.push("-c".to_owned());
        argv.push(format!("filter.{name}.required=false"));
    }
    argv.push("-C".to_owned());
    argv.push(call.cwd.clone());
    argv.extend(call.args.iter().cloned());
    Ok(argv)
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
            ..
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
                    "a branch named",
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

    fn repo_ok(&self, path: &Path) -> Result<(), String> {
        self.place_ok(path)?;
        let rel = path
            .strip_prefix(self.ctx.home)
            .map_err(|_| format!("{} isn't under {}", path.display(), self.ctx.home.display()))?;
        let parts: Vec<&str> = rel
            .components()
            .filter_map(|part| match part {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .collect();
        let refuse = |why: &str| Err(format!("{} {why}", path.display()));
        let Some(first) = parts.first() else {
            return refuse("is the home folder itself");
        };
        if first.starts_with('.') {
            return refuse("is in a hidden folder of the home folder, where a clone could change how this machine behaves");
        }
        let shared_cloud = parts.len() >= 3
            && same(first, "Library")
            && (same(parts[1], "CloudStorage") || same(parts[1], "Mobile Documents"));
        if (same(first, "Library") && !shared_cloud) || same(first, "Applications") {
            return refuse("is in a system folder of the home folder");
        }
        if parts.iter().any(|part| same(part, ".git")) {
            return refuse("is inside a .git folder");
        }
        if parts
            .iter()
            .any(|part| same(part, "bin") || same(part, "sbin"))
        {
            return refuse("has a bin folder in its path, where a clone could shadow a command");
        }
        if let Some(dir) = self
            .ctx
            .path_dirs
            .iter()
            .find(|dir| dir.starts_with(self.ctx.home) && folded_starts_with(dir, path))
        {
            return Err(format!(
                "{} is, or holds, {} on this machine's PATH, where a clone could shadow a command",
                path.display(),
                dir.display()
            ));
        }
        Ok(())
    }

    fn origin_here(&self, origin: &str) -> Result<(), String> {
        let local = match origin.strip_prefix("file://") {
            Some(rest) => Some(rest),
            None => origin.starts_with('/').then_some(origin),
        };
        let Some(local) = local else {
            return Ok(());
        };
        let path = Path::new(local);
        let plain = path.is_absolute()
            && path
                .components()
                .all(|part| matches!(part, Component::RootDir | Component::Normal(_)));
        if plain && path.starts_with(self.ctx.home) {
            Ok(())
        } else {
            Err(format!(
                "{origin} is a folder on the primary, not under {}: restore clones only from a remote or a folder here",
                self.ctx.home.display()
            ))
        }
    }

    fn nested(&self, named: &[&SessionEntry]) -> Vec<String> {
        let mut places: Vec<(&Path, Option<&Path>)> = Vec::new();
        for entry in named {
            if let (Movable::Yes, Some(facts)) = (&entry.movable, entry.repo.as_ref()) {
                places.push((&facts.root, facts.main_repo.as_deref()));
                if let Some(main) = facts.main_repo.as_deref() {
                    places.push((main, None));
                }
            }
        }
        let mut found = Vec::new();
        for (inner, owner) in &places {
            for (outer, _) in &places {
                if inner != outer
                    && folded_starts_with(inner, outer)
                    && !owner.is_some_and(|owner| folded_eq(owner, outer))
                {
                    found.push(format!(
                        "{} is inside {}, and only a linked worktree may be inside its own main repository",
                        inner.display(),
                        outer.display()
                    ));
                }
            }
        }
        found.sort();
        found.dedup();
        found
    }

    fn file_ok(&self, path: &Path) -> Result<(), String> {
        self.place_ok(path)?;
        match self
            .roots
            .iter()
            .find(|root| folded_starts_with(path, root))
        {
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
        self.repo_ok(root)?;
        self.origin_here(origin)?;
        if !Path::new(&entry.cwd).starts_with(root) {
            return Err(format!(
                "{id} can't move: its folder {} isn't inside its repository {}",
                entry.cwd,
                root.display()
            ));
        }
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
            PathState::Foreign => Err(format!(
                "{} on {} isn't a repository",
                root.display(),
                self.node()
            )),
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
                self.repo_ok(main)?;
                let state = if self.cloned.contains(main) {
                    PathState::Changed
                } else {
                    (self.probe)(main)
                };
                match state {
                    PathState::Missing => {
                        return Err(format!(
                            "{} isn't on {}: {} is a linked worktree of it",
                            main.display(),
                            self.node(),
                            root.display()
                        ));
                    }
                    PathState::Foreign => {
                        return Err(format!(
                            "{} on {} isn't a repository: {} is a linked worktree of it",
                            main.display(),
                            self.node(),
                            root.display()
                        ));
                    }
                    PathState::Clean { .. } | PathState::Changed => {}
                }
                self.push_repo(RestoreStep::AddWorktree {
                    main: main.clone(),
                    origin: origin.to_owned(),
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

    fn session_path(&self, entry: &SessionEntry, path: &Path, shape: Shape) -> Result<(), String> {
        self.file_ok(path)?;
        if shaped(
            entry.tool,
            &entry.id,
            &entry.transcripts,
            self.ctx.home,
            path,
            shape,
        ) {
            return Ok(());
        }
        Err(format!(
            "{} can't move: {} isn't a file the session keeps",
            entry.id,
            path.display()
        ))
    }

    fn transcript(&mut self, entry: &SessionEntry, path: &Path) -> Result<(), String> {
        self.session_path(entry, path, Shape::Transcript)?;
        let id = &entry.id;
        let Some(part) = self.part(path, |kind| matches!(kind, PartKind::Tail { .. })) else {
            return Err(format!(
                "{id} can't move: its transcript {} isn't in the set",
                path.display()
            ));
        };
        let PartKind::Tail { from, .. } = part.kind else {
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
        let shape = if part.kind == PartKind::Dir {
            Shape::Dir
        } else {
            Shape::File
        };
        self.session_path(entry, path, shape)?;
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
        let data = self.ctx.cluster.parent().unwrap_or(self.ctx.cluster);
        let db = self.ctx.cluster.join(CLUSTER_DB);
        let pages = data.join("pages");
        let devices = bridge_devices(data);
        for part in &self.m.parts {
            if owned.contains(&part.path) {
                continue;
            }
            let Some(from) = self.store.stored(&self.m.set, &part.path) else {
                self.refuse(format!("{} can't be stored", part.path.display()));
                continue;
            };
            let to = part.path.clone();
            let sha256 = part.sha256.clone();
            let step = match &part.kind {
                PartKind::Db if part.path == db => {
                    Some(RestoreStep::RestoreDb { from, to, sha256 })
                }
                PartKind::File if part.path == devices => {
                    Some(RestoreStep::MergeDevices { from, to, sha256 })
                }
                PartKind::Dir if part.path == pages => Some(RestoreStep::PutDir {
                    from,
                    to,
                    sha256,
                    len: part.len,
                }),
                _ => None,
            };
            let Some(step) = step else {
                self.refuse(format!(
                    "{} is a shared part this machine doesn't restore",
                    part.path.display()
                ));
                continue;
            };
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
    for why in planner.nested(&named) {
        planner.refuse(why);
    }
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
