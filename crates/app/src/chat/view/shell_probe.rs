use std::collections::HashMap;
use std::process::{Command, Stdio};

use gpui::Context;
use notchling_engine::transcript::shells::parse_writers_by_name;

use super::now_ms;
use super::ChatView;
use crate::chat::shell_list::{lsof_calls, probe_paths, PROBE_CHUNK};

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

    pub fn writing(&self, path: &str) -> Option<bool> {
        self.writers.get(path).map(|pids| !pids.is_empty())
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
        self.shells = self.shell_page.show(
            self.selected.as_deref(),
            &self.conversation,
            more,
            &|path| probe.writing(path),
        );
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
    let paths = probe_paths(&view.conversation, &view.shells, &|path| {
        view.shell_probe.writing(path)
    });
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
            let before = view.shells.clone();
            view.refresh_shells(0);
            if changed || view.shells != before {
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
        assert_eq!(probe.writing("/tmp/a.output"), None);
        probe.writers.insert("/tmp/a.output".into(), Vec::new());
        assert!(probe.writers("/tmp/a.output").is_empty());
        assert_eq!(probe.writing("/tmp/a.output"), Some(false));
        probe.writers.insert("/tmp/a.output".into(), vec![42]);
        assert_eq!(probe.writing("/tmp/a.output"), Some(true));
    }
}
