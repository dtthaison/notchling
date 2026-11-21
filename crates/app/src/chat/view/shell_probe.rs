use std::collections::{HashMap, HashSet};
use std::process::{Command, Stdio};

use gpui::Context;
use notchling_engine::transcript::shells::parse_writers_by_name;
use notchling_engine::transcript::shells_guard::session_owner;
use notchling_engine::transcript::shells_stop::{process_table, stop_shell, StopRequest, GRACE};

use super::now_ms;
use super::ChatView;
use crate::chat::shell_list::{lsof_calls, probe_paths_with_fan, PROBE_CHUNK};

const EVERY_MS: i64 = 3000;
const LSOF: &str = "/usr/sbin/lsof";

#[derive(Default)]
pub struct Probe {
    writers: HashMap<String, Vec<i32>>,
    last_ms: i64,
    busy: bool,
    stopped: HashSet<String>,
    stopping: HashSet<String>,
    minute: i64,
    clock: u64,
    settled: HashMap<String, u64>,
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
    }

    pub(super) fn shell_probe_stopped(&self, path: &str) -> bool {
        self.shell_probe.stopped.contains(path)
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
                probe.stopping.remove(&path);
                probe.settle(&path);
                if let Ok(outcome) = outcome {
                    if outcome.survivors == 0 {
                        if outcome.targets > 0 {
                            probe.stopped.insert(path.clone());
                        }
                        probe.writers.insert(path, Vec::new());
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
            .spawn(async move { look(&paths) })
            .await;
        let _ = this.update(cx, |view, cx| {
            let probe = &mut view.shell_probe;
            probe.busy = false;
            let changed = probe.apply(began, found);
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
