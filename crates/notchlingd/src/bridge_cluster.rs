use crate::cluster_prompts::RemotePrompts;
use crate::cluster_sessions::RemoteSessions;
use crate::cluster_switches::LateHandler;
use notchling_engine::bridge::model::{PendingPrompt, PromptKind, PromptQuestion};
use notchling_engine::bridge::relay::Relay;
use notchling_engine::sessions::Agent;
use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode, Tier};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const FOLLOW_EVERY_MS: u64 = 60_000;
pub const FOLLOW_RETRY_MS: u64 = 5_000;
pub const FOLLOWED_MAX: usize = 4;

struct Parts {
    handler: Arc<LateHandler>,
    sessions: Arc<RemoteSessions>,
    prompts: Arc<RemotePrompts>,
}

pub struct PhoneCluster {
    parts: Mutex<Option<Arc<Parts>>>,
    relay: Mutex<Option<Arc<dyn Relay>>>,
    followed: Mutex<HashMap<String, u64>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn the_phone() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

impl PhoneCluster {
    pub fn none() -> Arc<PhoneCluster> {
        Arc::new(PhoneCluster {
            parts: Mutex::new(None),
            relay: Mutex::new(None),
            followed: Mutex::new(HashMap::new()),
        })
    }

    pub fn attach(
        &self,
        handler: Arc<LateHandler>,
        sessions: Arc<RemoteSessions>,
        prompts: Arc<RemotePrompts>,
    ) {
        let mut slot = lock(&self.parts);
        if slot.is_none() {
            *slot = Some(Arc::new(Parts {
                handler,
                sessions,
                prompts,
            }));
        }
    }

    pub fn detach(&self) {
        let parts = lock(&self.parts).take();
        let relay = lock(&self.relay).take();
        drop((parts, relay));
    }

    pub fn attach_relay(&self, relay: Arc<dyn Relay>) {
        let mut slot = lock(&self.relay);
        if slot.is_none() {
            *slot = Some(relay);
        }
    }

    pub fn relay(&self) -> Option<Arc<dyn Relay>> {
        lock(&self.relay).clone()
    }

    pub fn attached(&self) -> bool {
        lock(&self.parts).is_some()
    }

    pub fn agents(&self) -> Vec<Agent> {
        let Some(parts) = lock(&self.parts).clone() else {
            return Vec::new();
        };
        parts
            .sessions
            .claude_rows()
            .into_iter()
            .filter_map(|row| serde_json::from_value::<Agent>(row).ok())
            .filter(|agent| agent.node.is_some())
            .collect()
    }

    pub fn prompts(&self) -> Vec<PendingPrompt> {
        let Some(parts) = lock(&self.parts).clone() else {
            return Vec::new();
        };
        parts.prompts.list().iter().filter_map(pending).collect()
    }

    pub fn call(&self, op: &str, args: &Value) -> Result<Value, ApiError> {
        let parts = lock(&self.parts).clone().ok_or_else(|| {
            ApiError::new(
                ErrorCode::Failed,
                "the cluster isn't running in this daemon",
            )
        })?;
        parts.handler.get()?.call(&the_phone(), op, args)
    }

    #[cfg(test)]
    pub(crate) fn followed_len(&self) -> usize {
        lock(&self.followed).len()
    }

    pub fn follow(&self, id: &str, now_ms: u64) {
        {
            let mut followed = lock(&self.followed);
            if followed.get(id).is_some_and(|at| now_ms < *at) {
                return;
            }
            if followed.len() >= FOLLOWED_MAX {
                followed.retain(|_, until| *until > now_ms);
            }
            while followed.len() >= FOLLOWED_MAX {
                let oldest = followed.iter().min_by_key(|(_, until)| **until);
                match oldest.map(|(id, _)| id.clone()) {
                    Some(id) => followed.remove(&id),
                    None => break,
                };
            }
            followed.insert(id.to_string(), now_ms + FOLLOW_RETRY_MS);
        }
        if self.call(ops::CLUSTER_FOLLOW, &json!({"id": id})).is_ok() {
            lock(&self.followed).insert(id.to_string(), now_ms + FOLLOW_EVERY_MS);
        }
    }
}

fn text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn questions(prompt: &Value) -> Vec<PromptQuestion> {
    prompt
        .get("question")
        .and_then(|q| q.get("items"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|item| PromptQuestion {
            header: text(item.get("header")),
            question: text(item.get("question")),
            options: item
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|o| o.as_str().map(str::to_string))
                .collect(),
            multi_select: item.get("multiSelect").and_then(Value::as_bool) == Some(true),
        })
        .collect()
}

pub fn pending(prompt: &Value) -> Option<PendingPrompt> {
    let id = prompt.get("id")?.as_str()?.to_string();
    let session_id = prompt
        .get("rowId")
        .and_then(Value::as_str)
        .filter(|row| !row.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| text(prompt.get("sessionId")));
    let kind = match prompt.get("kind").and_then(Value::as_str)? {
        "question" => {
            let items = questions(prompt);
            if items.is_empty() {
                return None;
            }
            PromptKind::Question(items)
        }
        _ => PromptKind::Permission,
    };
    Some(PendingPrompt {
        id,
        session_id,
        kind,
        tool_name: text(prompt.get("toolName")),
        summary: text(prompt.get("summary")),
        created_ms: prompt
            .get("createdMs")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
    })
}

#[cfg(test)]
#[path = "bridge_cluster_tests.rs"]
mod tests;
