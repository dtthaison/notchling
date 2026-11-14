use std::collections::HashSet;
use std::sync::Arc;

use super::antigravity::AntigravityProvider;
use super::claude::ClaudeProvider;
use super::codex::CodexProvider;
use super::flows::delegate_jobs;
use super::subagents::{Glance, Roots, SessionRef};
use super::{Capability, Provider, Support, Tool};
use crate::delegate::record;
use crate::tools::codex::children::{ChildSource, DayFolders};
use crate::transcript::model::{Conversation, Timestamp};
use crate::transcript::subagents::Subagent;

pub const MAX_DEPTH: u8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub tool: Tool,
    pub capability: Capability,
    pub support: Support,
    pub wired: bool,
}

impl Cell {
    pub fn entry(&self) -> String {
        format!("{} {}", self.tool.name(), self.capability.id())
    }
}

pub struct Registry {
    providers: Vec<Box<dyn Provider>>,
}

impl Registry {
    pub fn standard() -> Registry {
        Registry::with_sources(Arc::new(DayFolders))
    }

    pub fn with_sources(children: Arc<dyn ChildSource>) -> Registry {
        Registry {
            providers: vec![
                Box::new(ClaudeProvider),
                Box::new(CodexProvider::new(children)),
                Box::new(AntigravityProvider),
            ],
        }
    }

    pub fn of(&self, tool: Tool) -> &dyn Provider {
        self.providers[tool.index()].as_ref()
    }

    pub fn support(&self, tool: Tool, c: Capability) -> Support {
        self.of(tool).support(c)
    }

    pub fn is_wired(&self, tool: Tool, c: Capability) -> bool {
        self.of(tool).wired().contains(&c)
    }

    pub fn cells(&self) -> Vec<Cell> {
        let mut out = Vec::with_capacity(Tool::ALL.len() * Capability::ALL.len());
        for tool in Tool::ALL {
            for capability in Capability::ALL {
                out.push(Cell {
                    tool,
                    capability,
                    support: self.support(tool, capability),
                    wired: self.is_wired(tool, capability),
                });
            }
        }
        out
    }
}

impl Registry {
    pub fn subagents(&self, top: &SessionRef, roots: &Roots, now: Timestamp) -> Vec<Subagent> {
        let records = delegate_jobs::read_folder(&roots.delegations);
        let mut seen: HashSet<String> = HashSet::from([top.id.clone()]);
        let mut out = Vec::new();
        self.walk(top, 0, &records, roots, now, &mut seen, &mut out);
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &self,
        parent: &SessionRef,
        depth: u8,
        records: &[record::Delegation],
        roots: &Roots,
        now: Timestamp,
        seen: &mut HashSet<String>,
        out: &mut Vec<Subagent>,
    ) {
        if depth >= MAX_DEPTH {
            return;
        }
        let mut rows = self
            .of(parent.tool)
            .subagents()
            .list(parent, roots, now)
            .unwrap_or_default();
        rows.extend(delegate_jobs::subagents(
            records,
            parent,
            roots,
            now,
            &record::pid_alive,
        ));
        rows.sort_by(|a, b| a.started.total_cmp(&b.started));
        for mut row in rows {
            if !seen.insert(row.id.clone()) {
                continue;
            }
            row.session = parent.session.clone();
            row.parent = parent.id.clone();
            row.depth = depth + 1;
            let child = row.as_parent();
            out.push(row);
            self.walk(&child, depth + 1, records, roots, now, seen, out);
        }
    }

    pub fn glance(&self, s: &Subagent) -> Option<Glance> {
        self.of(s.origin.tool).subagents().glance(s).ok()
    }

    pub fn open(&self, s: &Subagent) -> Option<Arc<Conversation>> {
        self.of(s.origin.tool).subagents().open(s).ok()
    }
}
