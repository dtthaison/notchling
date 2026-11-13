use std::collections::HashMap;
use std::process::{Command, Stdio};

use gpui::Context;
use notchling_engine::transcript::shells::parse_writers_by_name;

use super::now_ms;
use super::ChatView;
use crate::chat::shell_list::{lsof_calls, probe_paths};

const EVERY_MS: i64 = 3000;
const LSOF: &str = "/usr/sbin/lsof";

#[derive(Default)]
pub struct Probe {
    writers: HashMap<String, Vec<i32>>,
    last_ms: i64,
    busy: bool,
}

impl Probe {
    pub fn writers(&self, path: &str) -> Vec<i32> {
        self.writers.get(path).cloned().unwrap_or_else(|| vec![0])
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
    let wanted: Vec<(String, Vec<String>)> = paths.iter().map(|p| names_of(p)).collect();
    let mut found: HashMap<String, Vec<i32>> = HashMap::new();
    for args in lsof_calls(paths) {
        let text = Command::new(LSOF)
            .args(&args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default();
        for (path, pids) in parse_writers_by_name(&text, &wanted) {
            if args.contains(&path) {
                found.insert(path, pids);
            }
        }
    }
    found
}

impl ChatView {
    pub(super) fn refresh_shells(&mut self, more: usize) {
        self.shells = self
            .shell_page
            .show(self.selected.as_deref(), &self.conversation, more);
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
    let now = now_ms();
    if now.saturating_sub(view.shell_probe.last_ms) < EVERY_MS {
        return;
    }
    let paths = probe_paths(&view.conversation, &view.shells);
    if paths.is_empty() {
        return;
    }
    view.shell_probe.last_ms = now;
    view.shell_probe.busy = true;
    cx.spawn(async move |this, cx| {
        let found = cx
            .background_executor()
            .spawn(async move { look(&paths) })
            .await;
        let _ = this.update(cx, |view, cx| {
            let probe = &mut view.shell_probe;
            probe.busy = false;
            let mut changed = false;
            for (path, writers) in found {
                if probe.writers.get(&path) != Some(&writers) {
                    probe.writers.insert(path, writers);
                    changed = true;
                }
            }
            if changed {
                cx.notify();
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
        probe.writers.insert("/tmp/a.output".into(), Vec::new());
        assert!(probe.writers("/tmp/a.output").is_empty());
    }
}
