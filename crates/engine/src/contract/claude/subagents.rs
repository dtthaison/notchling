use std::sync::Arc;

use crate::contract::subagents::{Glance, Roots, SessionRef, Subagents};
use crate::contract::{Capability, Unsupported};
use crate::transcript::model::{Conversation, Timestamp};
use crate::transcript::subagents::{self, Subagent};

pub struct ClaudeSubagents;

impl Subagents for ClaudeSubagents {
    fn list(
        &self,
        parent: &SessionRef,
        _roots: &Roots,
        now: Timestamp,
    ) -> Result<Vec<Subagent>, Unsupported> {
        let Some(path) = parent.transcript.as_ref().map(|p| p.to_string_lossy()) else {
            return Ok(Vec::new());
        };
        if parent.id != parent.session {
            return Ok(Vec::new());
        }
        Ok(subagents::list_across_clears(
            &path,
            &parent.name,
            &parent.finished,
            now,
        ))
    }

    fn glance(&self, s: &Subagent) -> Result<Glance, Unsupported> {
        if s.path.is_empty() {
            return Err(Unsupported::not_native(Capability::SubagentGlance));
        }
        let peeked = subagents::peek(s.origin, &s.path, s.is_running());
        Ok(Glance {
            tokens: peeked.as_ref().and_then(|p| p.context),
            recent: peeked.and_then(|p| p.recent),
        })
    }

    fn open(&self, s: &Subagent) -> Result<Arc<Conversation>, Unsupported> {
        Ok(subagents::read_shared(&s.path))
    }
}
