use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::shells::{lsof_args, parse_writers_by_name, NOT_WRITERS};
use super::shells_guard;
pub use super::shells_orphans::orphan_targets;

pub const GRACE: Duration = Duration::from_secs(3);

pub(super) const DEPTH: usize = 256;

const OTHER_TOOLS: [&str; 5] = ["notchling", "notchlingd", "codex", "agy", "antigravity"];

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

    pub(super) fn claude_titled(&self) -> bool {
        let named = |text: &str| text.split_whitespace().next() == Some("claude");
        named(self.name())
            || self
                .command
                .split_whitespace()
                .next()
                .is_some_and(|word| word.rsplit('/').next() == Some("claude"))
    }

    pub(super) fn protected(&self) -> bool {
        NOT_WRITERS.contains(&self.name())
            || self.claude_titled()
            || OTHER_TOOLS.contains(&self.name())
            || self.command.contains("/claude/")
            || self.command.contains("/claude-code/")
            || self.command.contains("Notchling.app")
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

pub(super) fn target_of(proc: &Proc) -> Target {
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
    command.env("LC_ALL", "C").env("TZ", "UTC");
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub targets: usize,
    pub killed: usize,
    pub survivors: usize,
    pub left: Vec<i32>,
}

impl Outcome {
    pub fn stopped(&self) -> bool {
        self.survivors == 0 && self.left.is_empty()
    }
}

pub fn left_after(mut read: impl FnMut() -> Vec<i32>, waited: bool, until: Instant) -> Vec<i32> {
    loop {
        let left = read();
        if left.is_empty() || !waited || Instant::now() >= until {
            return left;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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
            left: Vec::new(),
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
        left: Vec::new(),
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
    let text = path.to_string_lossy();
    let until = Instant::now() + grace;
    let writers = writers_of(&text);
    if writers.is_empty() {
        return Ok(Outcome::default());
    }
    let Some(table) = process_table() else {
        return Ok(Outcome {
            targets: writers.len(),
            killed: 0,
            survivors: writers.len(),
            left: writers,
        });
    };
    let found = stop_plan(&writers, &table, &scope, request.orphans);
    let outcome = execute(&found, &scope, grace);
    let left = left_after(|| writers_of(&text), !found.is_empty(), until);
    Ok(Outcome { left, ..outcome })
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
