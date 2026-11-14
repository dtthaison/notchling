use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use super::conformance::Checked;
use super::subagents::{Roots, SessionRef};
use super::{Capability, Registry, Tool};
use crate::transcript::model::Timestamp;
use crate::transcript::subagents::{Origin, Status, Subagent};
use crate::transcript::util::modified_secs;

const CODEX_PARENT: &str = "01a1d5da-0000-7000-8000-00000000000a";

struct World {
    parent: SessionRef,
    roots: Roots,
    now: Timestamp,
}

fn newest(dir: &Path, depth: usize) -> Timestamp {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0.0;
    };
    let mut best: Timestamp = 0.0;
    for entry in entries.flatten() {
        let path = entry.path();
        let at = if path.is_dir() && depth > 0 {
            newest(&path, depth - 1)
        } else {
            modified_secs(&path.to_string_lossy()).unwrap_or(0.0)
        };
        best = best.max(at);
    }
    best
}

fn world(tool: Tool, root: &Path) -> Option<World> {
    match tool {
        Tool::Claude => {
            let transcript = root.join("projects/-work-app/sess-1.jsonl");
            let finished: HashMap<String, String> =
                [("done1".to_string(), "completed".to_string())].into();
            Some(World {
                parent: SessionRef {
                    id: "sess-1".into(),
                    tool,
                    transcript: Some(transcript),
                    name: "worker".into(),
                    finished: Arc::new(finished),
                    session: "sess-1".into(),
                },
                roots: Roots::default(),
                now: newest(&root.join("projects"), 4) + 5.0,
            })
        }
        Tool::Codex => {
            let sessions = root.join("sessions");
            let day = sessions.join("2026/11/13");
            let transcript = day.join(format!("rollout-2026-11-13T09-00-00-{CODEX_PARENT}.jsonl"));
            Some(World {
                parent: SessionRef {
                    id: format!("codex:{CODEX_PARENT}"),
                    tool,
                    transcript: Some(transcript),
                    name: String::new(),
                    finished: Arc::default(),
                    session: format!("codex:{CODEX_PARENT}"),
                },
                roots: Roots {
                    codex_sessions: sessions,
                    ..Roots::default()
                },
                now: newest(&day, 0) + 5.0,
            })
        }
        Tool::Antigravity => None,
    }
}

fn rows(registry: &Registry, w: &World) -> Vec<Subagent> {
    registry
        .of(w.parent.tool)
        .subagents()
        .list(&w.parent, &w.roots, w.now)
        .unwrap_or_else(|e| panic!("subagent.list: {e:?}"))
}

pub fn check_list(registry: &Registry, tool: Tool, root: &Path) {
    let w = world(tool, root).unwrap_or_else(|| panic!("no fixture world for {}", tool.name()));
    let found = rows(registry, &w);
    assert_eq!(found.len(), 2, "{}: {found:?}", tool.name());
    for row in &found {
        assert!(
            row.id.starts_with(&format!("{}:", tool.name())),
            "{}",
            row.id
        );
        assert_eq!(row.origin, Origin::native(tool));
        assert!(row.started >= 0.0 && !row.description.is_empty());
    }
}

pub fn check_status(registry: &Registry, tool: Tool, root: &Path) {
    let w = world(tool, root).unwrap_or_else(|| panic!("no fixture world for {}", tool.name()));
    let found = rows(registry, &w);
    let count = |status: Status| found.iter().filter(|r| r.status == status).count();
    assert_eq!(count(Status::Running), 1, "{}: {found:?}", tool.name());
    assert_eq!(count(Status::Completed), 1, "{}: {found:?}", tool.name());
}

fn running(found: Vec<Subagent>) -> Subagent {
    found
        .into_iter()
        .find(|r| r.status == Status::Running)
        .expect("a running child")
}

pub fn check_glance(registry: &Registry, tool: Tool, root: &Path) {
    let w = world(tool, root).unwrap_or_else(|| panic!("no fixture world for {}", tool.name()));
    let child = running(rows(registry, &w));
    let glance = registry
        .glance(&child)
        .unwrap_or_else(|| panic!("{}: no glance", tool.name()));
    assert!(glance.tokens.is_some_and(|t| t > 0), "{}", tool.name());
}

pub fn check_open(registry: &Registry, tool: Tool, root: &Path) {
    let w = world(tool, root).unwrap_or_else(|| panic!("no fixture world for {}", tool.name()));
    let found = rows(registry, &w);
    for child in &found {
        let conversation = registry
            .open(child)
            .unwrap_or_else(|| panic!("{}: not opened", tool.name()));
        assert!(!conversation.items.is_empty(), "{}", child.id);
    }
}

pub fn check(registry: &Registry, tool: Tool, c: Capability, root: &Path) -> Checked {
    if world(tool, root).is_none() {
        return Checked::NotWritten;
    }
    match c {
        Capability::SubagentList => check_list(registry, tool, root),
        Capability::SubagentStatus => check_status(registry, tool, root),
        Capability::SubagentGlance => check_glance(registry, tool, root),
        Capability::SubagentOpen => check_open(registry, tool, root),
        _ => return Checked::NotWritten,
    }
    Checked::Passed
}
