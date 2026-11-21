use crate::ask_find::tail_text;
use crate::late_emit::LateSalt;
use notchling_engine::ask::evidence::Evidence;
use notchling_engine::ask::{candidates, find_by_evidence, Asker};
use notchling_events::kinds::agents::AgentNamed;
use notchling_events::kinds::choices::{AgentTool, AskerKind};
use notchling_events::spec::Outcome;
use notchling_events::{Emit, Event, Ref};
use notchling_local_api::{AgentCall, AgentIdentity};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

struct Queued {
    call: AgentCall,
    at_ms: i64,
}

struct Waiting {
    queued: Queued,
    due: Instant,
    tries: usize,
}

pub const RETRY_MS: [u64; 4] = [300, 1_000, 3_000, 6_000];

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
        AgentNames::spawn_with(
            emit,
            salt,
            sessions,
            RETRY_MS.map(Duration::from_millis).to_vec(),
        )
    }

    pub fn spawn_with(
        emit: Arc<dyn Emit>,
        salt: LateSalt,
        sessions: Arc<Mutex<Value>>,
        retries: Vec<Duration>,
    ) -> AgentNames {
        let (tx, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("notchling-agent-named".into())
            .spawn(move || work(&*emit, &salt, &sessions, &retries, &rx))
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

fn work(
    emit: &dyn Emit,
    salt: &LateSalt,
    sessions: &Mutex<Value>,
    retries: &[Duration],
    rx: &Receiver<Queued>,
) {
    let mut waiting: Vec<Waiting> = Vec::new();
    let mut open = true;
    while open || !waiting.is_empty() {
        let mut round: Vec<Waiting> = Vec::new();
        let incoming = match waiting.iter().map(|w| w.due).min() {
            Some(due) if open => rx.recv_timeout(due.saturating_duration_since(Instant::now())),
            Some(_) => Err(RecvTimeoutError::Disconnected),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match incoming {
            Ok(first) => {
                for queued in std::iter::once(first).chain(rx.try_iter()) {
                    round.push(Waiting {
                        queued,
                        due: Instant::now(),
                        tries: 0,
                    });
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => open = false,
        }
        let now = Instant::now();
        let (due, later): (Vec<Waiting>, Vec<Waiting>) = std::mem::take(&mut waiting)
            .into_iter()
            .partition(|w| !open || w.due <= now);
        waiting = later;
        round.extend(due);
        let mut seen: HashMap<Key, Label> = HashMap::new();
        for mut item in round {
            let who = &item.queued.call.identity;
            let key: Key = (
                format!("{:?}", who.tool),
                who.session.clone(),
                who.cmd_hash.clone(),
                who.out_hash.clone(),
            );
            let label = seen
                .entry(key)
                .or_insert_with(|| label_for(who, item.queued.at_ms, salt, sessions))
                .clone();
            if label.asker == AskerKind::Unknown && open && can_resolve_later(who) {
                if let Some(wait) = retries.get(item.tries) {
                    item.due = Instant::now() + *wait;
                    item.tries += 1;
                    waiting.push(item);
                    continue;
                }
            }
            let event = AgentNamed {
                asker: label.asker,
                agent: label.agent.as_deref().map(Ref::new),
            };
            item.queued.call.scope.clone().run(|| {
                let _ = emit.commit(Event::new(&event, Outcome::Info, item.queued.at_ms));
            });
        }
    }
}

fn can_resolve_later(who: &AgentIdentity) -> bool {
    who.tool == AgentTool::Claude
        && who.verified
        && who.session.is_some()
        && (who.cmd_hash.is_some() || who.out_hash.is_some())
}

pub fn label_for(
    who: &AgentIdentity,
    at_ms: i64,
    salt: &LateSalt,
    sessions: &Mutex<Value>,
) -> Label {
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
        AgentTool::Claude if who.verified => {}
        _ => return unknown,
    }
    let (Some(session), Some(salt)) = (who.session.as_deref(), salt.get()) else {
        return unknown;
    };
    let Some(transcript) = transcript_of(sessions, session) else {
        return unknown;
    };
    let all = candidates(&transcript, "", at_ms);
    let evidence = Evidence {
        salt: &salt.0,
        cmd_hash: who.cmd_hash.as_deref(),
        out_hash: who.out_hash.as_deref(),
        opened_ms: at_ms,
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
