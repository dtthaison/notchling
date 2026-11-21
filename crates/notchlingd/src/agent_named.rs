use crate::ask_find::tail_text;
use crate::late_emit::LateSalt;
use notchling_engine::ask::evidence::Evidence;
use notchling_engine::ask::{candidates, find_by_evidence, Asker};
use notchling_events::kinds::agents::AgentNamed;
use notchling_events::kinds::choices::{AgentTool, AskerKind};
use notchling_events::spec::Outcome;
use notchling_events::{Emit, Event, Ref};
use notchling_local_api::AgentCall;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

struct Queued {
    call: AgentCall,
    at_ms: i64,
}

pub struct AgentNames {
    tx: Mutex<Option<Sender<Queued>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub asker: AskerKind,
    pub agent: Option<String>,
}

type Key = (String, Option<String>, Option<String>, Option<String>);

impl AgentNames {
    pub fn spawn(emit: Arc<dyn Emit>, salt: LateSalt, sessions: Arc<Mutex<Value>>) -> AgentNames {
        let (tx, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("notchling-agent-named".into())
            .spawn(move || work(&*emit, &salt, &sessions, &rx))
            .ok();
        AgentNames {
            tx: Mutex::new(Some(tx)),
            thread: Mutex::new(thread),
        }
    }

    pub fn enqueue(&self, call: &AgentCall, at_ms: i64) {
        if let Some(tx) = self
            .tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            let _ = tx.send(Queued {
                call: call.clone(),
                at_ms,
            });
        }
    }

    pub fn close(&self) {
        self.tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(thread) = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = thread.join();
        }
    }
}

fn work(emit: &dyn Emit, salt: &LateSalt, sessions: &Mutex<Value>, rx: &Receiver<Queued>) {
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while let Ok(more) = rx.try_recv() {
            batch.push(more);
        }
        let mut seen: HashMap<Key, Label> = HashMap::new();
        for queued in batch {
            let who = &queued.call.identity;
            let key: Key = (
                format!("{:?}", who.tool),
                who.session.clone(),
                who.cmd_hash.clone(),
                who.out_hash.clone(),
            );
            let label = seen
                .entry(key)
                .or_insert_with(|| label_for(&queued, salt, sessions))
                .clone();
            let event = AgentNamed {
                asker: label.asker,
                agent: label.agent.as_deref().map(Ref::new),
            };
            queued.call.scope.clone().run(|| {
                let _ = emit.commit(Event::new(&event, Outcome::Info, queued.at_ms));
            });
        }
    }
}

fn label_for(queued: &Queued, salt: &LateSalt, sessions: &Mutex<Value>) -> Label {
    let who = &queued.call.identity;
    let unknown = Label {
        asker: AskerKind::Unknown,
        agent: None,
    };
    match who.tool {
        AgentTool::Codex => {
            return Label {
                asker: AskerKind::Codex,
                agent: None,
            }
        }
        AgentTool::Antigravity => {
            return Label {
                asker: AskerKind::Antigravity,
                agent: None,
            }
        }
        AgentTool::Claude => {}
        _ => return unknown,
    }
    let (Some(session), Some(salt)) = (who.session.as_deref(), salt.get()) else {
        return unknown;
    };
    let Some(transcript) = transcript_of(sessions, session) else {
        return unknown;
    };
    let all = candidates(&transcript, "", queued.at_ms);
    let evidence = Evidence {
        salt: &salt.0,
        cmd_hash: who.cmd_hash.as_deref(),
        out_hash: who.out_hash.as_deref(),
        opened_ms: queued.at_ms,
    };
    match find_by_evidence(&evidence, &all, tail_text) {
        Some(found) => Label {
            asker: match found.asker {
                Asker::Main => AskerKind::Main,
                Asker::Subagent => AskerKind::Subagent,
                _ => AskerKind::Unknown,
            },
            agent: found.agent_id,
        },
        None => unknown,
    }
}

fn transcript_of(sessions: &Mutex<Value>, session: &str) -> Option<PathBuf> {
    let list = sessions.lock().unwrap_or_else(PoisonError::into_inner);
    list.as_array()?
        .iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(session))
        .and_then(|row| row.get("transcriptPath").and_then(Value::as_str))
        .map(PathBuf::from)
}

#[cfg(test)]
#[path = "agent_named_tests.rs"]
mod tests;
