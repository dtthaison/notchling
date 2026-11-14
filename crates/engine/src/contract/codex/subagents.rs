use std::path::Path;
use std::sync::Arc;

use crate::contract::subagents::{Glance, Roots, SessionRef, Subagents};
use crate::contract::{Capability, Tool, Unsupported};
use crate::tools::codex::children::{ChildSource, Head};
use crate::transcript::codex::{self, Ending};
use crate::transcript::model::{Conversation, Timestamp};
use crate::transcript::subagents::{Origin, Status, Subagent, QUIET_AFTER};
use crate::transcript::util::file_times;

pub struct CodexSubagents {
    source: Arc<dyn ChildSource>,
}

impl CodexSubagents {
    pub fn new(source: Arc<dyn ChildSource>) -> CodexSubagents {
        CodexSubagents { source }
    }
}

fn status_of(path: &str, updated: Timestamp, now: Timestamp) -> Status {
    let fresh = now - updated < QUIET_AFTER;
    match codex::ending(path) {
        Ending::Completed => Status::Completed,
        Ending::Failed => Status::Failed,
        Ending::Stopped => Status::Stopped,
        Ending::Running | Ending::Unknown if fresh => Status::Running,
        Ending::Running | Ending::Unknown => Status::Quiet,
    }
}

fn row(head: &Head, now: Timestamp) -> Subagent {
    let path = head.path.to_string_lossy().into_owned();
    let (updated, _) = file_times(&path).unwrap_or((0.0, 0.0));
    let title = codex::meta(&path).and_then(|m| m.title);
    let kind = head.role.clone().or_else(|| head.nickname.clone());
    let description = title
        .or_else(|| head.nickname.clone())
        .unwrap_or_else(|| "Codex subagent".into());
    Subagent {
        id: format!("codex:{}", head.thread),
        status: status_of(&path, updated, now),
        path,
        description,
        agent_type: kind.unwrap_or_else(|| "Codex".into()),
        model: None,
        effort: None,
        background: false,
        tool_use_id: None,
        started: head.started,
        updated,
        origin: Origin::native(Tool::Codex),
        session: String::new(),
        parent: String::new(),
        depth: head.depth.max(1),
    }
}

impl Subagents for CodexSubagents {
    fn list(
        &self,
        parent: &SessionRef,
        _roots: &Roots,
        now: Timestamp,
    ) -> Result<Vec<Subagent>, Unsupported> {
        let (Tool::Codex, Some(path)) = (parent.tool, parent.transcript.as_deref()) else {
            return Ok(Vec::new());
        };
        let heads = self.source.children(parent.own_id(), path);
        Ok(heads.iter().map(|h| row(h, now)).collect())
    }

    fn glance(&self, s: &Subagent) -> Result<Glance, Unsupported> {
        if s.path.is_empty() || !Path::new(&s.path).is_file() {
            return Err(Unsupported::not_native(Capability::SubagentGlance));
        }
        let peeked = codex::peek(&s.path, s.is_running());
        Ok(Glance {
            tokens: peeked.as_ref().and_then(|p| p.context),
            recent: peeked.and_then(|p| p.recent),
        })
    }

    fn open(&self, s: &Subagent) -> Result<Arc<Conversation>, Unsupported> {
        Ok(codex::read_shared(&s.path))
    }
}
