use std::collections::{HashMap, HashSet};

use super::shells_stop::{target_of, Proc, Scope, Target, DEPTH};

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

fn shared(proc: &Proc) -> bool {
    let name = proc.name().to_ascii_lowercase();
    proc.command.contains(".app/Contents/")
        || SHARED.iter().any(|word| {
            name.strip_prefix(word)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(['-', ':', ' ']))
        })
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
            && !shared(p)
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
