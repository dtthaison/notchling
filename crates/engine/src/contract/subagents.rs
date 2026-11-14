use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Capability, Tool, Unsupported};
use crate::transcript::model::{Conversation, Timestamp};
use crate::transcript::subagents::{Subagent, Via};

#[derive(Clone, Debug)]
pub struct SessionRef {
    pub id: String,
    pub tool: Tool,
    pub transcript: Option<PathBuf>,
    pub name: String,
    pub finished: Arc<HashMap<String, String>>,
    pub session: String,
}

fn strip(id: &str) -> &str {
    match id.split_once(':') {
        Some(("claude" | "codex" | "antigravity" | "agy" | "job", rest)) => rest,
        _ => id,
    }
}

impl SessionRef {
    pub fn own_id(&self) -> &str {
        strip(&self.id)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Roots {
    pub codex_sessions: PathBuf,
    pub antigravity: PathBuf,
    pub antigravity_copy: PathBuf,
    pub delegations: PathBuf,
}

pub struct Glance {
    pub tokens: Option<u64>,
    pub recent: Option<Conversation>,
}

pub trait Subagents: Send + Sync {
    fn list(
        &self,
        _parent: &SessionRef,
        _roots: &Roots,
        _now: Timestamp,
    ) -> Result<Vec<Subagent>, Unsupported> {
        Err(Unsupported::not_native(Capability::SubagentList))
    }

    fn glance(&self, _s: &Subagent) -> Result<Glance, Unsupported> {
        Err(Unsupported::not_native(Capability::SubagentGlance))
    }

    fn open(&self, _s: &Subagent) -> Result<Arc<Conversation>, Unsupported> {
        Err(Unsupported::not_native(Capability::SubagentOpen))
    }
}

pub struct Unported;

impl Subagents for Unported {}

pub static UNPORTED: Unported = Unported;

fn brain_session(path: &Path) -> Option<String> {
    path.ancestors()
        .nth(3)
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
}

fn rollout_thread(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let chars: Vec<char> = stem.chars().collect();
    (chars.len() >= 36).then(|| chars[chars.len() - 36..].iter().collect())
}

impl Subagent {
    pub fn as_parent(&self) -> SessionRef {
        let tool = self.origin.tool;
        let path = (!self.path.is_empty()).then(|| PathBuf::from(&self.path));
        let id = match (self.origin.via, &path) {
            (Via::Delegated, Some(p)) => match tool {
                Tool::Codex => rollout_thread(p).map(|t| format!("codex:{t}")),
                Tool::Antigravity => brain_session(p).map(|c| format!("antigravity:{c}")),
                Tool::Claude => None,
            },
            _ => None,
        }
        .unwrap_or_else(|| self.id.clone());
        SessionRef {
            id,
            tool,
            transcript: path,
            name: self.description.clone(),
            finished: Arc::default(),
            session: self.session.clone(),
        }
    }
}
