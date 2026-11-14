use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::contract::subagents::{Glance, Roots, SessionRef, Subagents};
use crate::contract::{Capability, Tool, Unsupported};
use crate::tools::agy::children::{self, Signals};
use crate::tools::agy::elsewhere::{self, log_path};
use crate::tools::agy::summaries::{self, ChildRow};
use crate::transcript::model::{Conversation, Timestamp};
use crate::transcript::subagents::{Origin, Status, Subagent};

const REUSE: Duration = Duration::from_secs(5);
const GENERIC: &str = "self";

pub struct AntigravitySubagents;

fn status_of(row: &ChildRow, log: &Path, heard: &HashSet<String>) -> Status {
    if row.killed {
        return Status::Stopped;
    }
    if row.status.contains("RUNNING") {
        return Status::Running;
    }
    let Signals { sent, failed } = children::signals(log);
    if failed {
        Status::Failed
    } else if sent || heard.contains(&row.id) {
        Status::Completed
    } else {
        Status::Quiet
    }
}

fn description(role: Option<&str>, name: &str) -> String {
    match role.filter(|r| !r.is_empty()) {
        Some(role) => role.to_owned(),
        None if name.is_empty() || name == GENERIC => "Antigravity subagent".into(),
        None => name.to_owned(),
    }
}

impl Subagents for AntigravitySubagents {
    fn list(
        &self,
        parent: &SessionRef,
        roots: &Roots,
        _now: Timestamp,
    ) -> Result<Vec<Subagent>, Unsupported> {
        if parent.tool != Tool::Antigravity {
            return Ok(Vec::new());
        }
        let own = parent.own_id();
        let rows =
            summaries::children_cached(&roots.antigravity, &roots.antigravity_copy, REUSE, own);
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let parent_log = log_path(&roots.antigravity, own);
        let heard = children::heard_from(&parent_log);
        let roles = children::roles(&parent_log);
        let mut made: Vec<(Timestamp, ChildRow, Subagent)> = rows
            .into_iter()
            .map(|row| {
                let log = log_path(&roots.antigravity, &row.id);
                let started = children::started(&log).unwrap_or(row.modified);
                let status = status_of(&row, &log, &heard);
                let path = if log.is_file() {
                    log.to_string_lossy().into_owned()
                } else {
                    String::new()
                };
                let agent = Subagent {
                    id: format!("antigravity:{}", row.id),
                    path,
                    description: String::new(),
                    agent_type: if row.agent_name.is_empty() {
                        "Antigravity".into()
                    } else {
                        row.agent_name.clone()
                    },
                    model: None,
                    effort: None,
                    background: false,
                    tool_use_id: None,
                    started,
                    updated: row.modified.max(started),
                    status,
                    origin: Origin::native(Tool::Antigravity),
                    session: String::new(),
                    parent: String::new(),
                    depth: row.depth.max(1),
                };
                (started, row, agent)
            })
            .collect();
        made.sort_by(|a, b| a.0.total_cmp(&b.0));
        let by_order = roles.len() == made.len();
        Ok(made
            .into_iter()
            .enumerate()
            .map(|(n, (_, row, mut agent))| {
                let role = by_order.then(|| roles[n].as_str());
                agent.description = description(role, &row.agent_name);
                agent
            })
            .collect())
    }

    fn glance(&self, s: &Subagent) -> Result<Glance, Unsupported> {
        if s.path.is_empty() || !Path::new(&s.path).is_file() {
            return Err(Unsupported::not_native(Capability::SubagentGlance));
        }
        let peeked = elsewhere::peek(Path::new(&s.path), s.is_running());
        Ok(Glance {
            tokens: peeked.as_ref().and_then(|p| p.context),
            recent: peeked.and_then(|p| p.recent),
        })
    }

    fn open(&self, s: &Subagent) -> Result<Arc<Conversation>, Unsupported> {
        if s.path.is_empty() || !Path::new(&s.path).is_file() {
            return Err(Unsupported::not_native(Capability::SubagentOpen));
        }
        Ok(Arc::new(
            elsewhere::from_log_file(Path::new(&s.path), None).0,
        ))
    }
}
