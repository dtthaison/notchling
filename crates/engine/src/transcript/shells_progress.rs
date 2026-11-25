use std::collections::{HashMap, HashSet};
use std::process::{Command, Stdio};

pub const RUNNING_AFTER_S: u64 = 20 * 60;
pub const QUIET_FOR_S: u64 = 10 * 60;
pub const MAX_GAP_S: u64 = 90;
pub const IDLE_CPU_MS_PER_MIN: u64 = 300;
pub const SNOOZE_S: u64 = 30 * 60;
const KEPT_S: u64 = QUIET_FOR_S + 2 * MAX_GAP_S;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub at: u64,
    pub cpu_ms: u64,
    pub out_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuProc {
    pub pid: i32,
    pub ppid: i32,
    pub cpu_ms: u64,
}

pub fn parse_cpu_time(text: &str) -> Option<u64> {
    let (days, rest) = match text.split_once('-') {
        Some((d, rest)) => (d.parse::<u64>().ok()?, rest),
        None => (0, text),
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [m, s] => (0, m.parse::<u64>().ok()?, *s),
        [h, m, s] => (h.parse::<u64>().ok()?, m.parse::<u64>().ok()?, *s),
        _ => return None,
    };
    let seconds: f64 = seconds.parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let whole = ((days * 24 + hours) * 60 + minutes) * 60;
    Some(whole * 1000 + (seconds * 1000.0).round() as u64)
}

pub fn parse_cpu_table(text: &str) -> Vec<CpuProc> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let pid = words.next()?.parse().ok()?;
            let ppid = words.next()?.parse().ok()?;
            let cpu_ms = parse_cpu_time(words.next()?)?;
            Some(CpuProc { pid, ppid, cpu_ms })
        })
        .collect()
}

pub fn cpu_table() -> Option<Vec<CpuProc>> {
    let out = Command::new("/bin/ps")
        .env("LC_ALL", "C")
        .args(["-axo", "pid=,ppid=,time="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let table = parse_cpu_table(&String::from_utf8_lossy(&out.stdout));
    (out.status.success() && !table.is_empty()).then_some(table)
}

pub fn tree_cpu_ms(table: &[CpuProc], roots: &[i32]) -> u64 {
    let mut members: HashSet<i32> = roots.iter().copied().collect();
    loop {
        let before = members.len();
        for p in table {
            if members.contains(&p.ppid) {
                members.insert(p.pid);
            }
        }
        if members.len() == before {
            break;
        }
    }
    table
        .iter()
        .filter(|p| members.contains(&p.pid))
        .map(|p| p.cpu_ms)
        .sum()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoProgress {
    pub quiet_s: u64,
}

impl NoProgress {
    pub fn minutes(self) -> u64 {
        self.quiet_s / 60
    }
}

pub fn judge(history: &[Sample], now: u64, running_s: Option<u64>) -> Option<NoProgress> {
    if running_s? <= RUNNING_AFTER_S {
        return None;
    }
    let last = history.last()?;
    if now.saturating_sub(last.at) > MAX_GAP_S {
        return None;
    }
    let window_start = last.at.saturating_sub(QUIET_FOR_S);
    let inside: Vec<&Sample> = history.iter().filter(|s| s.at >= window_start).collect();
    let first = inside.first()?;
    if first.at > window_start + MAX_GAP_S {
        return None;
    }
    let before = history.iter().rfind(|s| s.at < window_start);
    let start = before.copied().unwrap_or(**first);
    let mut prev = start;
    for s in inside.iter().copied().copied() {
        if s.at.saturating_sub(prev.at) > MAX_GAP_S
            || s.cpu_ms < prev.cpu_ms
            || s.out_bytes != start.out_bytes
        {
            return None;
        }
        prev = s;
    }
    let span = last.at.saturating_sub(start.at);
    if span < QUIET_FOR_S {
        return None;
    }
    let spent = last.cpu_ms.saturating_sub(start.cpu_ms);
    let allowed = IDLE_CPU_MS_PER_MIN * span / 60;
    (spent <= allowed).then_some(NoProgress { quiet_s: span })
}

#[derive(Debug, Default)]
pub struct Tracker {
    history: HashMap<String, Vec<Sample>>,
    snoozed: HashMap<String, u64>,
}

impl Tracker {
    pub fn record(&mut self, key: &str, sample: Sample) {
        let kept = self.history.entry(key.to_owned()).or_default();
        if kept.last().is_some_and(|last| {
            sample.at.saturating_sub(last.at) > MAX_GAP_S || sample.at < last.at
        }) {
            kept.clear();
        }
        kept.push(sample);
        let cut = sample.at.saturating_sub(KEPT_S);
        let drop = kept
            .iter()
            .take_while(|s| s.at < cut)
            .count()
            .saturating_sub(1);
        kept.drain(..drop);
    }

    pub fn forget(&mut self, key: &str) {
        self.history.remove(key);
    }

    pub fn forget_except(&mut self, live: &HashSet<String>) {
        self.history.retain(|key, _| live.contains(key));
        self.snoozed.retain(|key, _| live.contains(key));
    }

    pub fn snooze(&mut self, key: &str, now: u64) {
        self.snoozed.insert(key.to_owned(), now + SNOOZE_S);
    }

    pub fn snoozed(&self, key: &str, now: u64) -> bool {
        self.snoozed.get(key).is_some_and(|until| now < *until)
    }

    pub fn verdict(&self, key: &str, now: u64, running_s: Option<u64>) -> Option<NoProgress> {
        judge(self.history.get(key)?, now, running_s)
    }
}

#[cfg(test)]
#[path = "shells_progress_tests.rs"]
mod tests;
