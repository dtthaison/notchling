use crate::control::Command;
use crate::git::read::{command, driver_names};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoFacts {
    pub root: PathBuf,
    pub origin: Option<String>,
    pub branch: Option<String>,
    pub head: String,
    pub upstream: Option<String>,
    pub unpushed: u32,
    pub dirty: bool,
    pub untracked: bool,
    pub submodules: bool,
    pub main_repo: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCall {
    folder: String,
    sub: Vec<String>,
    drivers: Vec<String>,
}

impl GitCall {
    fn new(folder: &Path, sub: &[&str]) -> Self {
        GitCall {
            folder: folder.to_string_lossy().into_owned(),
            sub: sub.iter().map(|a| (*a).to_owned()).collect(),
            drivers: Vec::new(),
        }
    }

    fn built(&self) -> Command {
        let sub: Vec<&str> = self.sub.iter().map(String::as_str).collect();
        command(&self.folder, &self.drivers, &sub).with_env("GIT_OPTIONAL_LOCKS", Some("0"))
    }

    pub fn args(&self) -> Vec<String> {
        self.built().argv.split_off(1)
    }

    pub fn env(&self) -> BTreeMap<String, Option<String>> {
        self.built().env
    }

    pub fn to_command(&self) -> Command {
        self.built()
    }
}

pub fn read_plan(folder: &Path) -> Vec<GitCall> {
    vec![
        GitCall::new(
            folder,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--show-toplevel",
                "--git-dir",
                "--git-common-dir",
            ],
        ),
        GitCall::new(
            folder,
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=normal",
                "--ignore-submodules=all",
            ],
        ),
        GitCall::new(folder, &["rev-list", "--count", "@{upstream}..HEAD"]),
        GitCall::new(folder, &["remote", "get-url", "origin"]),
        GitCall::new(folder, &["ls-files", "-z", "-s", "-v"]),
    ]
}

pub fn filter_probe(folder: &Path) -> GitCall {
    GitCall::new(folder, &["config", "-z", "--get-regexp", "^filter\\."])
}

pub fn without_filters(calls: Vec<GitCall>, probe_stdout: &str) -> Result<Vec<GitCall>, String> {
    let names = driver_names(probe_stdout);
    if names.iter().any(|n| n.contains('=') || n.contains('\n')) {
        return Err("a filter name git can't be told to skip".to_owned());
    }
    Ok(calls
        .into_iter()
        .map(|mut call| {
            call.drivers = names.clone();
            call
        })
        .collect())
}

struct Layout {
    root: PathBuf,
    main_repo: Option<PathBuf>,
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

fn layout(out: &GitOutput) -> Result<Option<Layout>, String> {
    if out.code == 128 && out.stderr.contains("not a git repository") {
        return Ok(None);
    }
    if out.code != 0 {
        return Err(format!(
            "git rev-parse exited {}: {}",
            out.code,
            first_line(&out.stderr)
        ));
    }
    let lines: Vec<&str> = out.stdout.lines().collect();
    let [root, git_dir, common] = lines[..] else {
        return Err("git rev-parse answered something unexpected".to_owned());
    };
    let main_repo = (git_dir != common).then(|| {
        let common = Path::new(common);
        let ends_in_git = common.file_name().is_some_and(|n| n == ".git");
        match common.parent() {
            Some(parent) if ends_in_git => parent.to_path_buf(),
            _ => common.to_path_buf(),
        }
    });
    Ok(Some(Layout {
        root: PathBuf::from(root),
        main_repo,
    }))
}

#[derive(Default)]
struct StatusFacts {
    head: String,
    branch: Option<String>,
    upstream: Option<String>,
    has_counts: bool,
    dirty: bool,
    untracked: bool,
}

fn status_facts(text: &str) -> StatusFacts {
    let mut facts = StatusFacts::default();
    let mut skip_next = false;
    for record in text.split('\0') {
        if std::mem::take(&mut skip_next) {
            continue;
        }
        if let Some(header) = record.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" if value != "(initial)" => facts.head = value.to_owned(),
                "branch.head" if value != "(detached)" => facts.branch = Some(value.to_owned()),
                "branch.upstream" => facts.upstream = Some(value.to_owned()),
                "branch.ab" => facts.has_counts = true,
                _ => {}
            }
        } else if record.starts_with("1 ") || record.starts_with("u ") {
            facts.dirty = true;
        } else if record.starts_with("2 ") {
            facts.dirty = true;
            skip_next = true;
        } else if record.starts_with("? ") {
            facts.untracked = true;
        }
    }
    if !facts.has_counts {
        facts.upstream = None;
    }
    facts
}

struct IndexFacts {
    hidden_edits: bool,
    submodules: bool,
}

fn index_facts(out: &GitOutput) -> Result<IndexFacts, String> {
    if out.code != 0 {
        return Err(format!(
            "git ls-files exited {}: {}",
            out.code,
            first_line(&out.stderr)
        ));
    }
    let mut facts = IndexFacts {
        hidden_edits: false,
        submodules: false,
    };
    for record in out.stdout.split('\0').filter(|r| !r.is_empty()) {
        let header = record.split('\t').next().unwrap_or("");
        let mut fields = header.split(' ');
        let tag = fields.next().unwrap_or("");
        if tag.chars().any(|c| c.is_ascii_lowercase() || c == 'S') {
            facts.hidden_edits = true;
        }
        if fields.next() == Some("160000") {
            facts.submodules = true;
        }
    }
    Ok(facts)
}

fn unpushed(out: &GitOutput, upstream: bool) -> Result<u32, String> {
    if !upstream {
        return Ok(0);
    }
    if out.code != 0 {
        return Err(format!("git rev-list exited {}", out.code));
    }
    out.stdout
        .trim()
        .parse()
        .map_err(|_| "git rev-list didn't answer a count".to_owned())
}

fn is_transport(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.' | '_'))
}

fn is_ssh(scheme: &str) -> bool {
    ["ssh", "git+ssh", "ssh+git"]
        .iter()
        .any(|ssh| scheme.eq_ignore_ascii_case(ssh))
}

fn without_credentials(url: &str) -> String {
    if let Some((helper, address)) = url.split_once("::") {
        if is_transport(helper) {
            return format!("{helper}::{}", without_credentials(address));
        }
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return scp_user_only(url);
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let Some((userinfo, host)) = authority.rsplit_once('@') else {
        return url.to_owned();
    };
    let user = userinfo.split(':').next().unwrap_or("");
    if is_ssh(scheme) && !user.is_empty() {
        format!("{scheme}://{user}@{host}{tail}")
    } else {
        format!("{scheme}://{host}{tail}")
    }
}

fn scp_user_only(url: &str) -> String {
    let Some(colon) = url.find(':') else {
        return url.to_owned();
    };
    let (prefix, path) = url.split_at(colon);
    if prefix.contains('/') {
        return url.to_owned();
    }
    match prefix.rsplit_once('@') {
        Some((user, host)) => format!("{}@{host}{path}", user.split(':').next().unwrap_or("")),
        None => url.to_owned(),
    }
}

fn origin(out: &GitOutput) -> Option<String> {
    if out.code != 0 {
        return None;
    }
    let url = out.stdout.trim();
    (!url.is_empty()).then(|| without_credentials(url))
}

pub fn parse(folder: &Path, outputs: &[GitOutput]) -> Result<Option<RepoFacts>, String> {
    let [top, status, ahead, remote, files] = outputs else {
        return Err(format!(
            "{} git answers for {}, wanted 5",
            outputs.len(),
            folder.display()
        ));
    };
    let Some(layout) = layout(top)? else {
        return Ok(None);
    };
    if status.code != 0 {
        return Err(format!(
            "git status exited {}: {}",
            status.code,
            first_line(&status.stderr)
        ));
    }
    let facts = status_facts(&status.stdout);
    let index = index_facts(files)?;
    Ok(Some(RepoFacts {
        root: layout.root,
        origin: origin(remote),
        branch: facts.branch,
        head: facts.head,
        unpushed: unpushed(ahead, facts.upstream.is_some())?,
        upstream: facts.upstream,
        dirty: facts.dirty || index.hidden_edits,
        untracked: facts.untracked,
        submodules: index.submodules,
        main_repo: layout.main_repo,
    }))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Movable {
    Yes,
    No(String),
}

pub fn movable(f: Option<&RepoFacts>, folder: &str) -> Movable {
    let Some(f) = f else {
        return Movable::No(format!("{folder} isn't a git repository"));
    };
    let root = f.root.display();
    if f.dirty {
        return Movable::No(format!("{root} has uncommitted changes"));
    }
    if f.untracked {
        return Movable::No(format!("{root} has untracked files"));
    }
    if f.submodules {
        return Movable::No(format!("{root} has submodules"));
    }
    let Some(branch) = &f.branch else {
        return Movable::No(format!("{root} isn't on a branch"));
    };
    let on_origin = f
        .upstream
        .as_deref()
        .is_some_and(|u| u.starts_with("origin/"));
    if !on_origin || f.origin.is_none() {
        return Movable::No(format!("{branch} has no pushed upstream"));
    }
    if f.unpushed > 0 {
        return Movable::No(format!(
            "{branch} has {} commits that aren't pushed",
            f.unpushed
        ));
    }
    Movable::Yes
}
