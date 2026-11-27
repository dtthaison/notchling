use crate::acting_ops::{now_ms, ActingOps};
use crate::agy_ops::AgyOps;
use crate::codex_ops::CodexOps;
use crate::every_session::{tool_of, EverySession};
use crate::op_group::OpGroup;
use crate::outbox_ops::OutboxOps;
use crate::reads_ops::ReadsOps;
use crate::session_asks::{Directory, Runner, Target};
use notchling_engine::delegate::agy_trust::claude_trusts;
use notchling_engine::sessions::{Agent, AgentTarget, Roots};
use notchling_local_api::{ops, Caller, ClientKind, Tier};
use serde_json::Value;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

pub struct LiveDirectory {
    sessions: Arc<Mutex<Value>>,
    every: Arc<EverySession>,
    reads: Arc<ReadsOps>,
    roots: Roots,
}

impl LiveDirectory {
    pub fn new(
        sessions: Arc<Mutex<Value>>,
        every: Arc<EverySession>,
        reads: Arc<ReadsOps>,
        roots: Roots,
    ) -> LiveDirectory {
        LiveDirectory {
            sessions,
            every,
            reads,
            roots,
        }
    }

    fn agent(&self, id: &str) -> Option<Agent> {
        if matches!(tool_of(id), "codex" | "antigravity") {
            return self.every.agents(now_ms()).into_iter().find(|a| a.id == id);
        }
        let list = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        list.as_array()?
            .iter()
            .find(|row| row["id"].as_str() == Some(id))
            .and_then(|row| serde_json::from_value::<Agent>(row.clone()).ok())
    }
}

fn target_of(agent: Agent) -> Target {
    let mut also: Vec<String> = agent.resume_session_id.iter().cloned().collect();
    if let AgentTarget::Job(job) = &agent.target {
        also.push(job.clone());
    }
    if let Some(stem) = agent
        .transcript_path
        .as_deref()
        .and_then(|p| Path::new(p).file_stem())
        .and_then(|s| s.to_str())
    {
        also.push(stem.to_string());
    }
    let name = if agent.name.is_empty() {
        agent.project.clone()
    } else {
        agent.name.clone()
    };
    Target {
        tool: tool_of(&agent.id),
        id: agent.id,
        folder: agent.cwd,
        name,
        remote: agent.node.is_some(),
        also,
    }
}

impl Directory for LiveDirectory {
    fn find(&self, id: &str) -> Option<Target> {
        self.agent(id).map(target_of)
    }

    fn organization(&self, folder: &str) -> String {
        self.reads.folder_org(folder)
    }

    fn trusted(&self, tool: &str, folder: &str) -> bool {
        if tool != "claude" {
            return true;
        }
        let login = std::fs::read_to_string(notchling_engine::control::session::claude_json_for(
            &self.roots,
            None,
        ))
        .unwrap_or_default();
        let real = std::fs::canonicalize(folder).ok();
        claude_trusts(&login, Path::new(folder))
            || real.is_some_and(|real| claude_trusts(&login, &real))
    }

    fn folder_exists(&self, folder: &str) -> bool {
        Path::new(folder).is_dir()
    }
}

fn the_app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

pub fn runner(
    acting: Arc<ActingOps>,
    codex: Arc<CodexOps>,
    agy: Arc<AgyOps>,
    outbox: Arc<OutboxOps>,
) -> Runner {
    Arc::new(move |op: &str, args: &Value| {
        if op.starts_with("codex.") {
            codex.call(op, args)
        } else if op.starts_with("agy.") {
            agy.call(op, args)
        } else if op == ops::SESSION_SEND {
            outbox.call(&the_app(), op, args)
        } else {
            acting.call(&the_app(), op, args)
        }
    })
}
