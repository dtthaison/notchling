use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::shells::{lsof_args, parse_writers_by_name, NOT_WRITERS};

pub const GRACE: Duration = Duration::from_secs(3);

#[cfg(target_os = "macos")]
const LSOF: &str = "/usr/sbin/lsof";
#[cfg(not(target_os = "macos"))]
const LSOF: &str = "lsof";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: i32,
    pub pgid: i32,
    pub command: String,
}

impl Proc {
    pub fn name(&self) -> &str {
        self.command.rsplit('/').next().unwrap_or(&self.command)
    }

    fn protected(&self) -> bool {
        NOT_WRITERS.contains(&self.name())
            || self.command.contains("/claude/")
            || self.command.contains("Notchling.app")
    }
}

pub fn parse_ps(text: &str) -> Vec<Proc> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let (pid, rest) = line.split_once(char::is_whitespace)?;
            let (pgid, command) = rest.trim_start().split_once(char::is_whitespace)?;
            Some(Proc {
                pid: pid.parse().ok()?,
                pgid: pgid.parse().ok()?,
                command: command.trim().to_string(),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub pgid: i32,
    pub members: Vec<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub groups: Vec<Group>,
    pub pids: Vec<i32>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.pids.is_empty()
    }

    pub fn every_pid(&self) -> Vec<i32> {
        let mut all: Vec<i32> = self
            .groups
            .iter()
            .flat_map(|g| g.members.iter().copied())
            .chain(self.pids.iter().copied())
            .collect();
        all.sort_unstable();
        all.dedup();
        all
    }
}

pub fn plan(writers: &[i32], procs: &[Proc], own_pid: i32, own_pgid: i32) -> Plan {
    let mut plan = Plan::default();
    for &pid in writers {
        if pid <= 1 || pid == own_pid || plan.every_pid().contains(&pid) {
            continue;
        }
        let Some(proc) = procs.iter().find(|p| p.pid == pid) else {
            continue;
        };
        if proc.protected() {
            continue;
        }
        let members: Vec<&Proc> = procs.iter().filter(|p| p.pgid == proc.pgid).collect();
        let group_ok = proc.pgid > 1
            && proc.pgid != own_pgid
            && members.iter().all(|p| !p.protected() && p.pid != own_pid);
        if group_ok {
            plan.groups.push(Group {
                pgid: proc.pgid,
                members: members.iter().map(|p| p.pid).collect(),
            });
        } else {
            plan.pids.push(pid);
        }
    }
    plan
}

pub fn writers_of(path: &str) -> Vec<i32> {
    let Ok(out) = Command::new(LSOF)
        .args(lsof_args(&[path.to_string()]))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let mut names = vec![path.to_string()];
    if let Ok(real) = std::fs::canonicalize(path) {
        names.push(real.to_string_lossy().into_owned());
    }
    parse_writers_by_name(
        &String::from_utf8_lossy(&out.stdout),
        &[(path.to_string(), names)],
    )
    .remove(path)
    .unwrap_or_default()
}

pub fn process_table() -> Vec<Proc> {
    Command::new("ps")
        .args(["-axo", "pid=,pgid=,comm="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|out| parse_ps(&String::from_utf8_lossy(&out.stdout)))
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcome {
    pub targets: usize,
    pub killed: usize,
    pub survivors: usize,
}

#[cfg(unix)]
fn alive(pid: i32) -> bool {
    let present = unsafe { libc::kill(pid, 0) } == 0
        || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    present
        && Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map(|out| {
                let stat = String::from_utf8_lossy(&out.stdout);
                !stat.trim().is_empty() && !stat.trim().starts_with('Z')
            })
            .unwrap_or(true)
}

#[cfg(unix)]
fn signal(plan: &Plan, sig: i32, only_alive: bool) {
    for group in &plan.groups {
        if !only_alive || group.members.iter().any(|&p| alive(p)) {
            unsafe { libc::kill(-group.pgid, sig) };
        }
    }
    for &pid in &plan.pids {
        if !only_alive || alive(pid) {
            unsafe { libc::kill(pid, sig) };
        }
    }
}

#[cfg(unix)]
pub fn execute(plan: &Plan, grace: Duration) -> Outcome {
    let all = plan.every_pid();
    if all.is_empty() {
        return Outcome::default();
    }
    signal(plan, libc::SIGTERM, false);
    let until = Instant::now() + grace;
    while Instant::now() < until && all.iter().any(|&p| alive(p)) {
        std::thread::sleep(Duration::from_millis(100));
    }
    let left = all.iter().filter(|&&p| alive(p)).count();
    if left > 0 {
        signal(plan, libc::SIGKILL, true);
        std::thread::sleep(Duration::from_millis(200));
    }
    Outcome {
        targets: all.len(),
        killed: left,
        survivors: all.iter().filter(|&&p| alive(p)).count(),
    }
}

#[cfg(not(unix))]
pub fn execute(_: &Plan, _: Duration) -> Outcome {
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

pub fn stop_output(path: &str, grace: Duration) -> Outcome {
    let (pid, pgid) = own();
    let found = plan(&writers_of(path), &process_table(), pid, pgid);
    execute(&found, grace)
}

#[cfg(test)]
#[path = "shells_stop_tests.rs"]
mod tests;
