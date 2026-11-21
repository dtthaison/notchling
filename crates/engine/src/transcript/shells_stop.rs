use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::shells::{lsof_args, parse_writers_by_name, NOT_WRITERS};
use super::shells_guard;

pub const GRACE: Duration = Duration::from_secs(3);

const DEPTH: usize = 256;

const OTHER_TOOLS: [&str; 5] = ["notchling", "notchlingd", "codex", "agy", "antigravity"];

const SHARED: [&str; 8] = [
    "terminal",
    "iterm2",
    "ghostty",
    "alacritty",
    "kitty",
    "wezterm",
    "tmux",
    "screen",
];

#[cfg(target_os = "macos")]
const LSOF: &str = "/usr/sbin/lsof";
#[cfg(not(target_os = "macos"))]
const LSOF: &str = "lsof";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    pub pgid: i32,
    pub started: String,
    pub zombie: bool,
    pub command: String,
}

impl Proc {
    pub fn name(&self) -> &str {
        self.command.rsplit('/').next().unwrap_or(&self.command)
    }

    fn protected(&self) -> bool {
        NOT_WRITERS.contains(&self.name())
            || OTHER_TOOLS.contains(&self.name())
            || self.command.contains("/claude/")
            || self.command.contains("/claude-code/")
            || self.command.contains("Notchling.app")
    }

    fn shared(&self) -> bool {
        let name = self.name().to_ascii_lowercase();
        self.command.contains(".app/Contents/")
            || SHARED.iter().any(|word| {
                name.strip_prefix(word)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with(['-', ':', ' ']))
            })
    }
}

fn take(line: &str) -> Option<(&str, &str)> {
    let line = line.trim_start();
    let end = line.find(char::is_whitespace).unwrap_or(line.len());
    (end > 0).then(|| (&line[..end], &line[end..]))
}

pub fn parse_ps(text: &str) -> Vec<Proc> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<Proc> {
    let (pid, rest) = take(line)?;
    let (ppid, rest) = take(rest)?;
    let (pgid, rest) = take(rest)?;
    let (stat, mut rest) = take(rest)?;
    let mut started = Vec::new();
    for _ in 0..5 {
        let (word, tail) = take(rest)?;
        started.push(word);
        rest = tail;
    }
    Some(Proc {
        pid: pid.parse().ok()?,
        ppid: ppid.parse().ok()?,
        pgid: pgid.parse().ok()?,
        started: started.join(" "),
        zombie: stat.starts_with('Z'),
        command: rest.trim().to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    pub owner: i32,
    pub own_pid: i32,
    pub own_pgid: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub pid: i32,
    pub started: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub pgid: i32,
    pub members: Vec<Target>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub groups: Vec<Group>,
    pub pids: Vec<Target>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.pids.is_empty()
    }

    pub fn every_target(&self) -> Vec<Target> {
        let mut all: Vec<Target> = self
            .groups
            .iter()
            .flat_map(|g| g.members.iter().cloned())
            .chain(self.pids.iter().cloned())
            .collect();
        all.sort_by_key(|t| t.pid);
        all.dedup_by_key(|t| t.pid);
        all
    }

    pub fn every_pid(&self) -> Vec<i32> {
        self.every_target().iter().map(|t| t.pid).collect()
    }
}

fn descends(by_pid: &HashMap<i32, &Proc>, pid: i32, owner: i32) -> bool {
    let mut at = pid;
    for _ in 0..DEPTH {
        let Some(proc) = by_pid.get(&at) else {
            return false;
        };
        if proc.ppid == owner {
            return true;
        }
        if proc.ppid <= 1 || proc.ppid == at {
            return false;
        }
        at = proc.ppid;
    }
    false
}

fn group_is_ours(
    procs: &[Proc],
    by_pid: &HashMap<i32, &Proc>,
    pgid: i32,
    scope: &Scope,
) -> Option<Vec<i32>> {
    let owner_pgid = by_pid.get(&scope.owner).map(|p| p.pgid);
    if pgid <= 1
        || pgid == scope.own_pgid
        || pgid == scope.own_pid
        || pgid == scope.owner
        || Some(pgid) == owner_pgid
    {
        return None;
    }
    let members: Vec<&Proc> = procs.iter().filter(|p| p.pgid == pgid).collect();
    members
        .iter()
        .all(|p| {
            p.pid != scope.own_pid
                && p.pid != scope.owner
                && !p.protected()
                && descends(by_pid, p.pid, scope.owner)
        })
        .then(|| members.iter().map(|p| p.pid).collect())
}

fn target_of(proc: &Proc) -> Target {
    Target {
        pid: proc.pid,
        started: proc.started.clone(),
    }
}

pub fn plan(writers: &[i32], procs: &[Proc], scope: &Scope) -> Plan {
    let mut plan = Plan::default();
    if scope.owner <= 1 {
        return plan;
    }
    let by_pid: HashMap<i32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut targets: Vec<&Proc> = Vec::new();
    for &pid in writers {
        let Some(proc) = by_pid.get(&pid).copied() else {
            continue;
        };
        if pid <= 1
            || pid == scope.own_pid
            || pid == scope.owner
            || proc.zombie
            || proc.protected()
            || !descends(&by_pid, pid, scope.owner)
            || targets.iter().any(|t| t.pid == pid)
        {
            continue;
        }
        targets.push(proc);
    }
    for proc in &targets {
        if plan.every_pid().contains(&proc.pid) {
            continue;
        }
        let leader_is_target = targets.iter().any(|t| t.pid == proc.pgid);
        let members = leader_is_target
            .then(|| group_is_ours(procs, &by_pid, proc.pgid, scope))
            .flatten();
        match members {
            Some(members) => plan.groups.push(Group {
                pgid: proc.pgid,
                members: members
                    .iter()
                    .filter_map(|pid| by_pid.get(pid).copied())
                    .map(target_of)
                    .collect(),
            }),
            None => plan.pids.push(target_of(proc)),
        }
    }
    plan
}

fn names_of(path: &str) -> Vec<String> {
    let mut names = vec![path.to_string()];
    if let Ok(real) = std::fs::canonicalize(path) {
        let real = real.to_string_lossy().into_owned();
        if !names.contains(&real) {
            names.push(real);
        }
    }
    names
}

fn kept(live: &[&Proc], by_pid: &HashMap<i32, &Proc>, scope: &Scope) -> HashSet<i32> {
    let mut kept = HashSet::new();
    let starts = live
        .iter()
        .filter(|p| p.protected() || p.name().to_ascii_lowercase().starts_with("notchling"))
        .map(|p| p.pid)
        .chain([scope.own_pid, scope.owner]);
    for start in starts {
        let mut at = start;
        for _ in 0..DEPTH {
            if at <= 1 || !kept.insert(at) {
                break;
            }
            let Some(proc) = by_pid.get(&at) else {
                break;
            };
            at = proc.ppid;
        }
    }
    kept
}

fn below_writer(by_pid: &HashMap<i32, &Proc>, writing: &HashSet<i32>, pid: i32) -> bool {
    let mut at = pid;
    for _ in 0..DEPTH {
        let Some(proc) = by_pid.get(&at) else {
            return false;
        };
        if proc.ppid <= 1 || proc.ppid == at {
            return false;
        }
        if writing.contains(&proc.ppid) {
            return true;
        }
        at = proc.ppid;
    }
    false
}

pub fn orphan_targets(writers: &[i32], procs: &[Proc], scope: &Scope) -> Vec<Target> {
    let live: Vec<&Proc> = procs.iter().filter(|p| !p.zombie).collect();
    let by_pid: HashMap<i32, &Proc> = live.iter().map(|p| (p.pid, *p)).collect();
    let writing: HashSet<i32> = writers
        .iter()
        .copied()
        .filter(|pid| by_pid.contains_key(pid))
        .collect();
    let kept = kept(&live, &by_pid, scope);
    let fits = |p: &Proc| {
        p.pid > 1
            && p.pid != scope.own_pid
            && p.pid != scope.owner
            && !p.protected()
            && !p.shared()
            && !kept.contains(&p.pid)
            && (writing.contains(&p.pid) || below_writer(&by_pid, &writing, p.pid))
            && (p.ppid == 1 || by_pid.get(&p.ppid).is_some_and(|up| up.pgid == p.pgid))
            && live
                .iter()
                .filter(|c| c.ppid == p.pid)
                .all(|c| writing.contains(&c.pid) || c.pgid == p.pgid)
    };
    let mut pgids: Vec<i32> = writing
        .iter()
        .filter_map(|pid| by_pid.get(pid))
        .map(|p| p.pgid)
        .collect();
    pgids.sort_unstable();
    pgids.dedup();
    let mut targets: Vec<Target> = Vec::new();
    for pgid in pgids {
        if pgid <= 1 || pgid == scope.own_pgid || pgid == scope.own_pid || pgid == scope.owner {
            continue;
        }
        let mut members = live.iter().filter(|p| p.pgid == pgid);
        if members.all(|p| fits(p)) {
            targets.extend(
                live.iter()
                    .filter(|p| p.pgid == pgid && writing.contains(&p.pid))
                    .map(|p| target_of(p)),
            );
        }
    }
    targets.sort_by_key(|t| t.pid);
    targets
}

pub fn writers_of_many(paths: &[String]) -> HashMap<String, Vec<i32>> {
    let wanted: Vec<(String, Vec<String>)> = paths
        .iter()
        .map(|path| (path.clone(), names_of(path)))
        .collect();
    if paths.is_empty() {
        return HashMap::new();
    }
    let Ok(out) = Command::new(LSOF)
        .args(lsof_args(paths))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return wanted
            .into_iter()
            .map(|(path, _)| (path, Vec::new()))
            .collect();
    };
    parse_writers_by_name(&String::from_utf8_lossy(&out.stdout), &wanted)
}

pub fn writers_of(path: &str) -> Vec<i32> {
    writers_of_many(&[path.to_string()])
        .remove(path)
        .unwrap_or_default()
}

fn ps() -> Command {
    let mut command = Command::new("ps");
    command.env("LC_ALL", "C");
    command
}

pub fn process_table() -> Option<Vec<Proc>> {
    let out = ps()
        .args(["-axo", "pid=,ppid=,pgid=,stat=,lstart=,comm="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let procs = parse_ps(&String::from_utf8_lossy(&out.stdout));
    (out.status.success() && !procs.is_empty()).then_some(procs)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcome {
    pub targets: usize,
    pub killed: usize,
    pub survivors: usize,
}

fn here(table: &[Proc], target: &Target) -> bool {
    table
        .iter()
        .any(|p| p.pid == target.pid && p.started == target.started && !p.zombie)
}

#[cfg(unix)]
fn send(pid: i32, sig: i32) {
    if pid > 1 {
        unsafe { libc::kill(pid, sig) };
    }
}

#[cfg(unix)]
fn send_group(pgid: i32, scope: &Scope, sig: i32) {
    if pgid > 1 && pgid != scope.own_pgid && pgid != scope.own_pid && pgid != scope.owner {
        unsafe { libc::kill(-pgid, sig) };
    }
}

#[cfg(unix)]
fn term(plan: &Plan, scope: &Scope) {
    for group in &plan.groups {
        send_group(group.pgid, scope, libc::SIGTERM);
    }
    for target in &plan.pids {
        send(target.pid, libc::SIGTERM);
    }
}

#[cfg(unix)]
fn kill_left(plan: &Plan, scope: &Scope, table: &[Proc]) {
    let by_pid: HashMap<i32, &Proc> = table.iter().map(|p| (p.pid, p)).collect();
    for group in &plan.groups {
        let leader_same = group
            .members
            .iter()
            .find(|t| t.pid == group.pgid)
            .is_some_and(|leader| here(table, leader));
        if leader_same && group_is_ours(table, &by_pid, group.pgid, scope).is_some() {
            send_group(group.pgid, scope, libc::SIGKILL);
        } else {
            for target in group.members.iter().filter(|t| here(table, t)) {
                send(target.pid, libc::SIGKILL);
            }
        }
    }
    for target in plan.pids.iter().filter(|t| here(table, t)) {
        send(target.pid, libc::SIGKILL);
    }
}

#[cfg(unix)]
pub fn execute(plan: &Plan, scope: &Scope, grace: Duration) -> Outcome {
    let all = plan.every_target();
    if all.is_empty() {
        return Outcome::default();
    }
    term(plan, scope);
    let until = Instant::now() + grace;
    let mut table = process_table();
    while Instant::now() < until
        && table
            .as_ref()
            .is_none_or(|t| all.iter().any(|target| here(t, target)))
    {
        std::thread::sleep(Duration::from_millis(100));
        table = process_table();
    }
    let Some(table) = table else {
        return Outcome {
            targets: all.len(),
            killed: 0,
            survivors: all.len(),
        };
    };
    let left = all.iter().filter(|t| here(&table, t)).count();
    if left > 0 {
        kill_left(plan, scope, &table);
        std::thread::sleep(Duration::from_millis(200));
    }
    let survivors = process_table().map_or(all.len(), |t| {
        all.iter().filter(|target| here(&t, target)).count()
    });
    Outcome {
        targets: all.len(),
        killed: left,
        survivors,
    }
}

#[cfg(not(unix))]
pub fn execute(_: &Plan, _: &Scope, _: Duration) -> Outcome {
    Outcome::default()
}

#[cfg(unix)]
fn own() -> (i32, i32) {
    unsafe { (libc::getpid(), libc::getpgrp()) }
}

#[cfg(not(unix))]
fn own() -> (i32, i32) {
    (std::process::id() as i32, 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NotAShellOutput,
    OwnerUnknown,
}

impl Refusal {
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::NotAShellOutput => "not_a_shell_output",
            Refusal::OwnerUnknown => "owner_unknown",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StopRequest<'a> {
    pub session: &'a str,
    pub id: &'a str,
    pub path: &'a str,
    pub owner: Option<u32>,
    pub orphans: bool,
}

pub fn stop_shell(request: &StopRequest, grace: Duration) -> Result<Outcome, Refusal> {
    stop_shell_in(
        request,
        &shells_guard::tmp_roots(),
        shells_guard::current_uid(),
        grace,
    )
}

pub fn stop_shell_in(
    request: &StopRequest,
    roots: &[PathBuf],
    uid: u32,
    grace: Duration,
) -> Result<Outcome, Refusal> {
    let path = shells_guard::shell_output(request.path, request.session, request.id, roots, uid)
        .ok_or(Refusal::NotAShellOutput)?;
    let owner = request
        .owner
        .and_then(|pid| i32::try_from(pid).ok())
        .filter(|pid| *pid > 1);
    if owner.is_none() && !request.orphans {
        return Err(Refusal::OwnerUnknown);
    }
    let (own_pid, own_pgid) = own();
    let scope = Scope {
        owner: owner.unwrap_or(0),
        own_pid,
        own_pgid,
    };
    let writers = writers_of(&path.to_string_lossy());
    if writers.is_empty() {
        return Ok(Outcome::default());
    }
    let Some(table) = process_table() else {
        return Ok(Outcome {
            targets: writers.len(),
            killed: 0,
            survivors: writers.len(),
        });
    };
    let found = stop_plan(&writers, &table, &scope, request.orphans);
    Ok(execute(&found, &scope, grace))
}

pub fn stop_plan(writers: &[i32], procs: &[Proc], scope: &Scope, orphans: bool) -> Plan {
    let owner_alive = scope.owner > 1 && procs.iter().any(|p| p.pid == scope.owner && !p.zombie);
    if owner_alive || !orphans {
        return plan(writers, procs, scope);
    }
    Plan {
        groups: Vec::new(),
        pids: orphan_targets(writers, procs, scope),
    }
}

#[cfg(test)]
#[path = "shells_stop_tests.rs"]
mod tests;
