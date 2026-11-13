use std::collections::HashMap;
use std::process::{Command, Stdio};

use gpui::Context;
use notchling_engine::transcript::shells::parse_writers;

use super::now_ms;
use super::ChatView;

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

fn look(path: &str) -> Vec<i32> {
    Command::new(LSOF)
        .args(["-w", "-F", "pca", "--", path])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|out| parse_writers(&String::from_utf8_lossy(&out.stdout)))
        .unwrap_or_default()
}

pub fn tick(view: &mut ChatView, cx: &mut Context<ChatView>) {
    if view.details_hidden || view.shell_probe.busy {
        return;
    }
    let now = now_ms();
    if now.saturating_sub(view.shell_probe.last_ms) < EVERY_MS {
        return;
    }
    let paths: Vec<String> = view
        .conversation
        .shells
        .iter()
        .filter(|s| !view.conversation.finished_agents.contains_key(&s.id))
        .map(|s| s.output_path.clone())
        .collect();
    if paths.is_empty() {
        return;
    }
    view.shell_probe.last_ms = now;
    view.shell_probe.busy = true;
    cx.spawn(async move |this, cx| {
        let found = cx
            .background_executor()
            .spawn(async move {
                paths
                    .into_iter()
                    .map(|p| {
                        let w = look(&p);
                        (p, w)
                    })
                    .collect::<Vec<_>>()
            })
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
