use std::collections::{HashMap, HashSet};
use std::process::{Command, Stdio};

use gpui::Context;
use notchling_engine::transcript::shells::parse_writers_by_name;
use notchling_engine::transcript::shells_guard::session_owner;
use notchling_engine::transcript::shells_progress::{
    cpu_table, tree_cpu_ms, CpuProc, Sample, Tracker,
};
use notchling_engine::transcript::shells_stop::{
    process_table, stop_shell, Outcome, StopRequest, GRACE,
};
use notchling_engine::transcript::shells_stuck::running_secs;

use super::now_ms;
use super::ChatView;
use crate::chat::shell_list::{lsof_calls, probe_paths_with_fan, PROBE_CHUNK};

pub const NO_PROGRESS_ENV: &str = "NOTCHLING_RENDER_NO_PROGRESS";

fn forced_quiet() -> Option<u64> {
    static FORCED: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *FORCED.get_or_init(|| std::env::var(NO_PROGRESS_ENV).ok()?.parse().ok())
}

const EVERY_MS: i64 = 3000;
const LSOF: &str = "/usr/sbin/lsof";

#[derive(Default)]
pub struct Probe {
    writers: HashMap<String, Vec<i32>>,
    last_ms: i64,
    busy: bool,
    stopped: HashSet<String>,
    left: HashSet<String>,
    stopping: HashSet<String>,
    minute: i64,
    clock: u64,
    settled: HashMap<String, u64>,
    tracker: Tracker,
}

impl Probe {
    pub fn writers(&self, path: &str) -> Vec<i32> {
        self.writers.get(path).cloned().unwrap_or_else(|| vec![0])
    }

    pub fn writing(&self, path: &str) -> Option<bool> {
        self.writers.get(path).map(|pids| !pids.is_empty())
    }

    fn begin(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn settle(&mut self, path: &str) {
        self.clock += 1;
        self.settled.insert(path.to_owned(), self.clock);
    }

    fn record_stop(&mut self, path: &str, outcome: &Outcome) {
        self.stopping.remove(path);
        self.settle(path);
        if !outcome.left.is_empty() {
            self.stopped.remove(path);
            self.left.insert(path.to_owned());
            self.writers.insert(path.to_owned(), outcome.left.clone());
        } else if outcome.stopped() {
            self.left.remove(path);
            if outcome.targets > 0 {
                self.stopped.insert(path.to_owned());
            }
            self.writers.insert(path.to_owned(), Vec::new());
        }
    }

    fn sample(&mut self, now_s: u64, seen: &Seen) {
        for (path, _) in seen.writers.iter().filter(|(_, pids)| pids.is_empty()) {
            self.tracker.forget(path);
        }
        let Some(table) = &seen.cpu else {
            return;
        };
        for (path, pids) in seen.writers.iter().filter(|(_, pids)| !pids.is_empty()) {
            let Some(out_bytes) = seen.sizes.get(path) else {
                continue;
            };
            let roots: Vec<i32> = pids.iter().copied().filter(|p| *p > 0).collect();
            self.tracker.record(
                path,
                Sample {
                    at: now_s,
                    cpu_ms: tree_cpu_ms(table, &roots),
                    out_bytes: *out_bytes,
                },
            );
        }
    }

    fn quiet_minutes(&self, path: &str, started: Option<f64>, now_s: u64) -> Option<u64> {
        if self.writing(path) != Some(true) {
            return None;
        }
        let running = running_secs(started, now_s as f64);
        self.tracker
            .verdict(path, now_s, running)
            .map(|quiet| quiet.minutes())
    }

    fn apply(&mut self, began: u64, found: HashMap<String, Vec<i32>>) -> bool {
        let mut changed = false;
        for (path, writers) in found {
            let stale = self.stopping.contains(&path)
                || self.settled.get(&path).is_some_and(|at| *at > began);
            if stale {
                continue;
            }
            if self.writers.get(&path) != Some(&writers) {
                self.writers.insert(path, writers);
                changed = true;
            }
        }
        changed
    }
}

fn names_of(path: &str) -> (String, Vec<String>) {
    let mut names = vec![path.to_owned()];
    if let Ok(real) = std::fs::canonicalize(path) {
        names.push(real.to_string_lossy().into_owned());
    }
    (path.to_owned(), names)
}

struct Seen {
    writers: HashMap<String, Vec<i32>>,
    cpu: Option<Vec<CpuProc>>,
    sizes: HashMap<String, u64>,
}

fn see(paths: &[String]) -> Seen {
    let writers = look(paths);
    let sizes = paths
        .iter()
        .filter_map(|p| Some((p.clone(), std::fs::metadata(p).ok()?.len())))
        .collect();
    let cpu = writers
        .values()
        .any(|pids| !pids.is_empty())
        .then(cpu_table)
        .flatten();
    Seen {
        writers,
        cpu,
        sizes,
    }
}

fn look(paths: &[String]) -> HashMap<String, Vec<i32>> {
    let mut found = HashMap::new();
    for (chunk, args) in paths.chunks(PROBE_CHUNK).zip(lsof_calls(paths)) {
        let Ok(out) = Command::new(LSOF)
            .args(&args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        else {
            continue;
        };
        let wanted: Vec<(String, Vec<String>)> = chunk.iter().map(|p| names_of(p)).collect();
        found.extend(parse_writers_by_name(
            &String::from_utf8_lossy(&out.stdout),
            &wanted,
        ));
    }
    found
}

impl ChatView {
    pub(super) fn refresh_shells(&mut self, more: usize) {
        let probe = &self.shell_probe;
        self.shells = self.shell_page.show_with_fan(
            self.selected.as_deref(),
            &self.conversation,
            &self.fan_shells,
            more,
            &|path| probe.writing(path),
        );
        let now_s = (now_ms() / 1000).max(0) as u64;
        self.shells.quiet = self
            .conversation
            .shells
            .iter()
            .chain(self.fan_shells.iter())
            .filter(|shell| !shell.output_path.is_empty())
            .filter_map(|shell| {
                let forced = forced_quiet()
                    .filter(|_| !self.conversation.finished_agents.contains_key(&shell.id));
                let minutes = forced
                    .or_else(|| probe.quiet_minutes(&shell.output_path, shell.started, now_s))?;
                Some((shell.id.clone(), minutes))
            })
            .collect();
    }

    pub(super) fn shell_probe_stopped(&self, path: &str) -> bool {
        self.shell_probe.stopped.contains(path)
    }

    pub(super) fn shell_probe_left(&self, path: &str) -> bool {
        self.shell_probe.left.contains(path)
    }

    pub(super) fn stop_shell(&mut self, id: &str, path: &str, cx: &mut Context<Self>) {
        let Some(session) = self.selected.clone() else {
            return;
        };
        if id.is_empty() || path.is_empty() || !self.shell_probe.stopping.insert(path.to_owned()) {
            return;
        }
        let (id, path, roots) = (id.to_owned(), path.to_owned(), self.root.roots());
        cx.spawn(async move |this, cx| {
            let (target, shell) = (path.clone(), id.clone());
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let owner =
                        process_table().and_then(|table| session_owner(&roots, &session, &table));
                    stop_shell(
                        &StopRequest {
                            session: &session,
                            id: &shell,
                            path: &target,
                            owner,
                            orphans: true,
                        },
                        GRACE,
                    )
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                let probe = &mut view.shell_probe;
                match outcome {
                    Ok(outcome) => probe.record_stop(&path, &outcome),
                    Err(_) => {
                        probe.stopping.remove(&path);
                        probe.settle(&path);
                    }
                }
                view.refresh_shells(0);
                view.redraw_details(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn load_more_shells(&mut self, cx: &mut Context<Self>) {
        self.refresh_shells(1);
        self.shell_probe.last_ms = 0;
        self.redraw_details(cx);
    }
}

pub fn tick(view: &mut ChatView, cx: &mut Context<ChatView>) {
    if view.details_hidden || view.shell_probe.busy {
        return;
    }
    if view
        .selected_session()
        .is_some_and(|s| crate::chat::remote::is_remote(&s.agent))
    {
        return;
    }
    let now = now_ms();
    if now.saturating_sub(view.shell_probe.last_ms) < EVERY_MS {
        return;
    }
    let paths = probe_paths_with_fan(
        &view.conversation,
        &view.fan_shells,
        &view.shells,
        &|path| view.shell_probe.writing(path),
    );
    if paths.is_empty() {
        return;
    }
    view.shell_probe.last_ms = now;
    view.shell_probe.busy = true;
    let began = view.shell_probe.begin();
    cx.spawn(async move |this, cx| {
        let found = cx
            .background_executor()
            .spawn(async move { see(&paths) })
            .await;
        let _ = this.update(cx, |view, cx| {
            let probe = &mut view.shell_probe;
            probe.busy = false;
            probe.sample((now_ms() / 1000).max(0) as u64, &found);
            let changed = probe.apply(began, found.writers);
            let before = view.shells.clone();
            view.refresh_shells(0);
            let minute = now_ms() / 60_000;
            let tick = view.shell_probe.minute != minute;
            view.shell_probe.minute = minute;
            if changed || view.shells != before || tick {
                cx.notify();
                if tick {
                    view.redraw_details(cx);
                }
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_not_looked_at_yet_counts_as_running() {
        let mut probe = Probe::default();
        assert_eq!(probe.writers("/tmp/a.output"), vec![0]);
        assert_eq!(probe.writing("/tmp/a.output"), None);
        probe.writers.insert("/tmp/a.output".into(), Vec::new());
        assert!(probe.writers("/tmp/a.output").is_empty());
        assert_eq!(probe.writing("/tmp/a.output"), Some(false));
        probe.writers.insert("/tmp/a.output".into(), vec![42]);
        assert_eq!(probe.writing("/tmp/a.output"), Some(true));
    }

    fn sampled(probe: &mut Probe, path: &str, now_s: u64, cpu_ms: u64, out_bytes: u64) {
        let seen = Seen {
            writers: HashMap::from([(path.to_owned(), vec![10])]),
            cpu: Some(vec![
                CpuProc {
                    pid: 10,
                    ppid: 1,
                    cpu_ms: cpu_ms / 2,
                },
                CpuProc {
                    pid: 11,
                    ppid: 10,
                    cpu_ms: cpu_ms - cpu_ms / 2,
                },
            ]),
            sizes: HashMap::from([(path.to_owned(), out_bytes)]),
        };
        probe.sample(now_s, &seen);
    }

    #[test]
    fn a_shell_whose_tree_sits_idle_and_writes_nothing_shows_no_progress() {
        let path = "/tmp/a.output";
        let mut probe = Probe::default();
        probe.writers.insert(path.into(), vec![10]);
        for step in 0..=60 {
            sampled(&mut probe, path, 1_000 + step * 12, 4_000, 77);
        }
        let started = Some(100.0);
        assert_eq!(probe.quiet_minutes(path, started, 1_720), Some(10));
        assert_eq!(probe.quiet_minutes(path, Some(900.0), 1_720), None);
        probe.writers.insert(path.into(), Vec::new());
        assert_eq!(probe.quiet_minutes(path, started, 1_720), None);
    }

    #[test]
    fn a_shell_that_keeps_writing_or_computing_shows_nothing() {
        let path = "/tmp/a.output";
        let mut probe = Probe::default();
        probe.writers.insert(path.into(), vec![10]);
        for step in 0..=60 {
            sampled(&mut probe, path, 1_000 + step * 12, 4_000, 77 + step);
        }
        assert_eq!(probe.quiet_minutes(path, Some(1.0), 1_720), None);
        let mut busy = Probe::default();
        busy.writers.insert(path.into(), vec![10]);
        for step in 0..=60 {
            sampled(&mut busy, path, 1_000 + step * 12, 4_000 + step * 5_000, 77);
        }
        assert_eq!(busy.quiet_minutes(path, Some(1.0), 1_720), None);
    }

    #[test]
    fn no_process_table_means_no_sample() {
        let path = "/tmp/a.output";
        let mut probe = Probe::default();
        let seen = Seen {
            writers: HashMap::from([(path.to_owned(), vec![10])]),
            cpu: None,
            sizes: HashMap::from([(path.to_owned(), 1)]),
        };
        probe.sample(5, &seen);
        assert!(probe.tracker.verdict(path, 5, Some(9_999)).is_none());
    }

    #[test]
    fn an_ended_shell_is_forgotten_even_when_no_shell_has_writers_and_no_table_was_read() {
        let path = "/tmp/a.output";
        let mut probe = Probe::default();
        probe.writers.insert(path.into(), vec![10]);
        for step in 0..=60 {
            sampled(&mut probe, path, 1_000 + step * 12, 4_000, 77);
        }
        assert!(probe.tracker.verdict(path, 1_720, Some(9_999)).is_some());
        let ended = Seen {
            writers: HashMap::from([(path.to_owned(), Vec::new())]),
            cpu: None,
            sizes: HashMap::from([(path.to_owned(), 77)]),
        };
        probe.sample(1_720, &ended);
        assert!(probe.tracker.verdict(path, 1_720, Some(9_999)).is_none());
    }

    fn seen(path: &str, pids: Vec<i32>) -> HashMap<String, Vec<i32>> {
        HashMap::from([(path.to_owned(), pids)])
    }

    #[test]
    fn a_probe_started_before_a_stop_cannot_bring_the_shell_back() {
        let path = "/tmp/a.output";
        let mut probe = Probe::default();
        probe.writers.insert(path.into(), vec![42]);
        let began = probe.begin();
        probe.stopping.insert(path.to_owned());
        assert!(!probe.apply(began, seen(path, vec![42])), "while stopping");
        probe.stopping.remove(path);
        probe.settle(path);
        probe.writers.insert(path.into(), Vec::new());
        assert!(!probe.apply(began, seen(path, vec![42])), "after the stop");
        assert_eq!(probe.writing(path), Some(false));
    }

    #[test]
    fn a_probe_started_after_a_stop_is_believed() {
        let path = "/tmp/a.output";
        let mut probe = Probe::default();
        probe.settle(path);
        probe.writers.insert(path.into(), Vec::new());
        let began = probe.begin();
        assert!(probe.apply(began, seen(path, vec![7])));
        assert_eq!(probe.writing(path), Some(true));
    }

    fn outcome(targets: usize, left: Vec<i32>) -> Outcome {
        Outcome {
            targets,
            killed: targets,
            survivors: 0,
            left,
        }
    }

    #[test]
    fn a_stop_that_left_writers_keeps_the_row_running_and_says_so() {
        let path = "/tmp/a.output";
        let mut probe = Probe::default();
        probe.writers.insert(path.into(), vec![40, 41]);
        let began = probe.begin();
        probe.stopping.insert(path.to_owned());
        probe.record_stop(path, &outcome(1, vec![41]));
        assert_eq!(probe.writing(path), Some(true));
        assert!(probe.left.contains(path));
        assert!(!probe.stopped.contains(path));
        assert!(!probe.stopping.contains(path));
        assert!(!probe.apply(began, seen(path, Vec::new())));
        assert_eq!(probe.writers(path), vec![41]);
    }

    #[test]
    fn a_stop_with_no_writers_left_is_stopped_and_clears_an_earlier_left() {
        let path = "/tmp/a.output";
        let mut probe = Probe::default();
        probe.record_stop(path, &outcome(1, vec![41]));
        probe.record_stop(path, &outcome(1, Vec::new()));
        assert_eq!(probe.writing(path), Some(false));
        assert!(probe.stopped.contains(path));
        assert!(!probe.left.contains(path));
    }

    #[test]
    fn a_probe_after_a_stop_still_shows_a_writer_that_came_back() {
        let path = "/tmp/a.output";
        let mut probe = Probe::default();
        probe.record_stop(path, &outcome(1, Vec::new()));
        let began = probe.begin();
        assert!(probe.apply(began, seen(path, vec![77])));
        assert_eq!(probe.writing(path), Some(true));
    }

    #[test]
    fn a_stop_leaves_other_shells_probe_results_alone() {
        let (a, b) = ("/tmp/a.output", "/tmp/b.output");
        let mut probe = Probe::default();
        let began = probe.begin();
        probe.settle(a);
        let mut found = seen(a, vec![1]);
        found.extend(seen(b, vec![2]));
        assert!(probe.apply(began, found));
        assert_eq!(probe.writing(a), None);
        assert_eq!(probe.writing(b), Some(true));
    }
}
