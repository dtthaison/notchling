use std::path::Path;

use serde::{Deserialize, Serialize};

use super::blob::{safe_name, sha256_of};

pub const NO_REMOTE: &str =
    "This repository has no remote; send its folder to the Worker another way";
pub const NO_NEW_COMMITS: &str = "There are no new commits to bring back";
pub const HARDENING: [&str; 7] = [
    "--no-optional-locks",
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "protocol.ext.allow=never",
];
pub const ENV: [(&str, &str); 3] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_PAGER", "cat"),
    ("LC_ALL", "C"),
];
pub const QUICK_SECS: u64 = 30;
pub const SLOW_SECS: u64 = 900;
const MAX_BRANCH: usize = 200;
const MAX_URL: usize = 2048;
const MAX_SLUG: usize = 40;
const MAX_NAME: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCall {
    pub cwd: String,
    pub args: Vec<String>,
    pub skip_if_exists: Option<String>,
    pub allow_failure: bool,
    pub refusals: Vec<(String, String)>,
    pub timeout_secs: u64,
}

impl GitCall {
    fn new(cwd: &str, args: &[&str]) -> GitCall {
        GitCall {
            cwd: cwd.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            skip_if_exists: None,
            allow_failure: false,
            refusals: Vec::new(),
            timeout_secs: QUICK_SECS,
        }
    }

    fn slow(mut self) -> GitCall {
        self.timeout_secs = SLOW_SECS;
        self
    }

    fn unless_present(mut self, path: &str) -> GitCall {
        self.skip_if_exists = Some(path.to_string());
        self
    }

    fn may_fail(mut self) -> GitCall {
        self.allow_failure = true;
        self
    }

    fn refuse(mut self, when: &str, text: &str) -> GitCall {
        self.refusals.push((when.to_string(), text.to_string()));
        self
    }

    pub fn argv(&self) -> Vec<String> {
        let mut argv: Vec<String> = HARDENING.iter().map(|a| a.to_string()).collect();
        argv.push("-C".to_string());
        argv.push(self.cwd.clone());
        argv.extend(self.args.iter().cloned());
        argv
    }
}

pub fn scrubbed(names: impl IntoIterator<Item = String>) -> Vec<String> {
    names
        .into_iter()
        .filter(|name| name.starts_with("GIT_"))
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoFacts {
    pub root: String,
    pub origin: Option<String>,
    pub branch: String,
    pub head: String,
    pub upstream: Option<String>,
    #[serde(default)]
    pub unpushed: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendPlan {
    pub bundle: Option<GitCall>,
}

fn hex_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn valid_branch(name: &str) -> bool {
    if name.is_empty()
        || name.len() > MAX_BRANCH
        || name.starts_with('-')
        || name.starts_with('/')
        || name.ends_with('/')
        || name.ends_with('.')
        || name == "@"
        || name == "HEAD"
        || name.contains("..")
        || name.contains("//")
        || name.contains("@{")
    {
        return false;
    }
    if name
        .chars()
        .any(|c| c.is_control() || c == ' ' || "~^:?*[\\".contains(c))
    {
        return false;
    }
    name.split('/')
        .all(|part| !part.starts_with('.') && !part.ends_with(".lock"))
}

pub fn origin_ok(url: &str) -> bool {
    if url.is_empty()
        || url.len() > MAX_URL
        || url.starts_with('-')
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return false;
    }
    if let Some((scheme, rest)) = url.split_once("://") {
        if !["https", "ssh", "git", "file"].contains(&scheme) {
            return false;
        }
        let authority = rest.split('/').next().unwrap_or("");
        return match authority.rsplit_once('@') {
            Some((userinfo, host)) => {
                !userinfo.contains(':') && !userinfo.starts_with('-') && !host.starts_with('-')
            }
            None => !authority.starts_with('-'),
        };
    }
    if url.starts_with('/') {
        return true;
    }
    match url.split_once(':') {
        Some((host, path)) => {
            !host.is_empty()
                && !host.starts_with('-')
                && !host.contains("@-")
                && !path.is_empty()
                && !path.starts_with(':')
                && !host.contains('/')
                && host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-@".contains(&b))
        }
        None => false,
    }
}

pub fn node_slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed: String = out.trim_matches('-').chars().take(MAX_SLUG).collect();
    let trimmed = trimmed.trim_end_matches('-');
    if trimmed.is_empty() {
        "node".to_string()
    } else {
        trimmed.to_string()
    }
}

fn path_ok(path: &str) -> bool {
    Path::new(path).is_absolute() && !path.contains('\0') && !path.starts_with('-')
}

fn repo_name(root: &str) -> String {
    let base = Path::new(root)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let mut name: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .take(MAX_NAME)
        .collect();
    if name.is_empty() || name.starts_with('.') {
        name.insert(0, '_');
    }
    name
}

fn checked(facts: &RepoFacts) -> Result<(), String> {
    if !valid_branch(&facts.branch) {
        return Err("The branch name isn't one git accepts".to_string());
    }
    if !hex_id(&facts.head) {
        return Err("The commit id isn't a full hash".to_string());
    }
    if facts.upstream.as_deref().is_some_and(|u| !hex_id(u)) {
        return Err("The upstream commit id isn't a full hash".to_string());
    }
    if let Some(origin) = &facts.origin {
        if !origin_ok(origin) {
            return Err(
                "The remote's address isn't one Notchling will clone from (no password in it, https, ssh, git or a path)"
                    .to_string(),
            );
        }
    }
    Ok(())
}

fn job_ok(job: &str) -> Result<(), String> {
    if safe_name(job) {
        Ok(())
    } else {
        Err("not a job name".to_string())
    }
}

fn root_ok(work_root: &str) -> Result<(), String> {
    if path_ok(work_root) {
        Ok(())
    } else {
        Err("the work folder must be an absolute path".to_string())
    }
}

pub fn repo_dir(facts: &RepoFacts, work_root: &str) -> Result<String, String> {
    root_ok(work_root)?;
    let origin = facts
        .origin
        .as_deref()
        .ok_or_else(|| NO_REMOTE.to_string())?;
    let hash = sha256_of(origin.as_bytes());
    Ok(format!(
        "{}/{}-{}",
        work_root.trim_end_matches('/'),
        repo_name(&facts.root),
        &hash[..8]
    ))
}

pub fn worktree_dir(job: &str, work_root: &str) -> Result<String, String> {
    job_ok(job)?;
    root_ok(work_root)?;
    Ok(format!("{}/jobs/{job}", work_root.trim_end_matches('/')))
}

pub fn plan_send(facts: &RepoFacts, out: &str) -> Result<SendPlan, String> {
    checked(facts)?;
    if !path_ok(out) {
        return Err("the bundle path must be absolute".to_string());
    }
    if facts.unpushed == 0 {
        return Ok(SendPlan { bundle: None });
    }
    let branch = format!("refs/heads/{}", facts.branch);
    let call = GitCall::new(
        &facts.root,
        &[
            "bundle",
            "create",
            out,
            &branch,
            "--not",
            "--remotes=origin",
        ],
    )
    .slow()
    .refuse("", "git couldn't bundle the commits to send");
    Ok(SendPlan { bundle: Some(call) })
}

pub fn plan_prepare(
    facts: &RepoFacts,
    job: &str,
    work_root: &str,
    bundle: Option<&str>,
) -> Result<Vec<GitCall>, String> {
    job_ok(job)?;
    root_ok(work_root)?;
    checked(facts)?;
    if facts.origin.is_none() {
        return Err(NO_REMOTE.to_string());
    }
    if bundle.is_some_and(|b| !path_ok(b)) {
        return Err("the bundle path must be absolute".to_string());
    }
    let origin = facts.origin.as_deref().unwrap_or("");
    let dir = repo_dir(facts, work_root)?;
    let tree = worktree_dir(job, work_root)?;
    let branch = &facts.branch;
    let mut calls = vec![
        GitCall::new(
            work_root,
            &["check-ref-format", &format!("refs/heads/{branch}")],
        )
        .refuse("", "The branch name isn't one git accepts"),
        GitCall::new(
            work_root,
            &["clone", "--no-checkout", "--no-tags", "--", origin, &dir],
        )
        .slow()
        .unless_present(&dir)
        .refuse("", "Couldn't clone the repository's remote on this machine"),
        GitCall::new(
            &dir,
            &[
                "fetch",
                "--no-tags",
                "--",
                "origin",
                "+refs/heads/*:refs/remotes/origin/*",
            ],
        )
        .slow()
        .refuse("", "Couldn't fetch the repository's remote on this machine"),
    ];
    if let Some(bundle) = bundle {
        calls.push(
            GitCall::new(
                &dir,
                &[
                    "fetch",
                    "--no-tags",
                    "--",
                    bundle,
                    &format!("+refs/heads/{branch}:refs/notchling/in/{job}"),
                ],
            )
            .refuse("", "The commits sent along couldn't be unpacked"),
        );
    }
    calls.push(
        GitCall::new(
            &dir,
            &["cat-file", "-e", &format!("{}^{{commit}}", facts.head)],
        )
        .refuse(
            "",
            "The commit isn't on the remote and no unpushed commits were sent",
        ),
    );
    calls.push(
        GitCall::new(
            &dir,
            &[
                "update-ref",
                &format!("refs/notchling/start/{job}"),
                &facts.head,
            ],
        )
        .unless_present(&tree),
    );
    calls.push(
        GitCall::new(
            &dir,
            &[
                "worktree",
                "add",
                "-B",
                &format!("notchling/{job}"),
                &tree,
                &facts.head,
            ],
        )
        .slow()
        .unless_present(&tree),
    );
    Ok(calls)
}

pub fn plan_cleanup(facts: &RepoFacts, job: &str, work_root: &str) -> Result<Vec<GitCall>, String> {
    let dir = repo_dir(facts, work_root)?;
    let tree = worktree_dir(job, work_root)?;
    let each = |args: &[&str]| GitCall::new(&dir, args).may_fail();
    Ok(vec![
        each(&["worktree", "remove", "--force", &tree]),
        each(&["worktree", "prune"]),
        each(&["branch", "-D", &format!("notchling/{job}")]),
        each(&["update-ref", "-d", &format!("refs/notchling/in/{job}")]),
        each(&["update-ref", "-d", &format!("refs/notchling/start/{job}")]),
    ])
}

pub fn plan_return(job: &str, work_root: &str, out: &str) -> Result<GitCall, String> {
    let tree = worktree_dir(job, work_root)?;
    if !path_ok(out) {
        return Err("the bundle path must be absolute".to_string());
    }
    Ok(GitCall::new(
        &tree,
        &[
            "bundle",
            "create",
            out,
            &format!("refs/notchling/start/{job}..refs/heads/notchling/{job}"),
        ],
    )
    .slow()
    .refuse("empty bundle", NO_NEW_COMMITS)
    .refuse("", "Couldn't bundle the work to bring back"))
}

pub fn back_branch(node_name: &str, job: &str) -> Result<String, String> {
    job_ok(job)?;
    Ok(format!("notchling/{}/{job}", node_slug(node_name)))
}

pub fn plan_fetch_back(
    root: &str,
    bundle: &str,
    node_name: &str,
    job: &str,
) -> Result<Vec<GitCall>, String> {
    job_ok(job)?;
    if !path_ok(root) || !path_ok(bundle) {
        return Err("the folder and the bundle must be absolute paths".to_string());
    }
    let branch = back_branch(node_name, job)?;
    Ok(vec![GitCall::new(
        root,
        &[
            "fetch",
            "--no-tags",
            "--",
            bundle,
            &format!("refs/heads/notchling/{job}:refs/heads/{branch}"),
        ],
    )
    .refuse("rejected", &format!("{branch} already has other commits"))
    .refuse(
        "",
        "Couldn't take the work back into this repository",
    )])
}

#[derive(Clone, Debug)]
pub struct FactsCalls {
    pub root: GitCall,
    pub head: GitCall,
    pub branch: GitCall,
    pub origin: GitCall,
    pub upstream: GitCall,
    pub unpushed: GitCall,
}

pub fn facts_calls(cwd: &str) -> Result<FactsCalls, String> {
    if !path_ok(cwd) {
        return Err("the folder must be an absolute path".to_string());
    }
    Ok(FactsCalls {
        root: GitCall::new(cwd, &["rev-parse", "--show-toplevel"]),
        head: GitCall::new(cwd, &["rev-parse", "--verify", "HEAD"]),
        branch: GitCall::new(cwd, &["symbolic-ref", "--short", "-q", "HEAD"]),
        origin: GitCall::new(cwd, &["remote", "get-url", "origin"]),
        upstream: GitCall::new(cwd, &["rev-parse", "--verify", "-q", "@{upstream}"]),
        unpushed: GitCall::new(
            cwd,
            &["rev-list", "--count", "HEAD", "--not", "--remotes=origin"],
        ),
    })
}

#[derive(Clone, Debug, Default)]
pub struct FactsOut {
    pub root: String,
    pub head: String,
    pub branch: Option<String>,
    pub origin: Option<String>,
    pub upstream: Option<String>,
    pub unpushed: Option<String>,
}

fn line(text: &str) -> String {
    text.trim_end_matches(['\n', '\r']).to_string()
}

pub fn parse_facts(out: FactsOut) -> Result<RepoFacts, String> {
    let branch = out.branch.map(|b| line(&b)).filter(|b| !b.is_empty());
    let Some(branch) = branch else {
        return Err("Check out a branch first: this repository is on a detached HEAD".to_string());
    };
    let unpushed = out
        .unpushed
        .as_deref()
        .and_then(|n| line(n).parse::<u32>().ok())
        .ok_or_else(|| "git couldn't count the commits the remote lacks".to_string())?;
    let facts = RepoFacts {
        root: line(&out.root),
        origin: out.origin.map(|o| line(&o)).filter(|o| !o.is_empty()),
        branch,
        head: line(&out.head),
        upstream: out.upstream.map(|u| line(&u)).filter(|u| !u.is_empty()),
        unpushed,
    };
    checked(&facts)?;
    if !path_ok(&facts.root) {
        return Err("git didn't say where the repository is".to_string());
    }
    Ok(facts)
}
