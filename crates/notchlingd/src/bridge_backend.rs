use crate::acting_ops::{ActingOps, Room};
use crate::catalog_ops::CatalogService;
use crate::prompts::{Kind, Prompt};
use notchling_engine::bridge::agents;
use notchling_engine::bridge::live::{Acting, Sources};
use notchling_engine::bridge::model::{
    Control, ModelChoice, PendingPrompt, PromptKind, PromptQuestion, PromptResponse, Session,
    TakeoverOutcome,
};
use notchling_engine::bridge::BridgeFailure;
use notchling_engine::catalog::{Tier as ModelTier, Tool};
use notchling_engine::sessions::{Agent, AgentTarget};
use notchling_local_api::{ops, topics, ApiError, Caller, ClientKind, ErrorCode, Tier};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const APPROVAL_WAIT: Duration = Duration::from_secs(120);

pub struct Approvals {
    room: Arc<dyn Room>,
    waiting: Mutex<HashMap<String, SyncSender<bool>>>,
    next: Mutex<u64>,
    wait: Duration,
}

impl Approvals {
    pub fn new(room: Arc<dyn Room>, wait: Duration) -> Approvals {
        Approvals {
            room,
            waiting: Mutex::new(HashMap::new()),
            next: Mutex::new(0),
            wait,
        }
    }

    pub fn request(&self, name: &str, platform: &str, address: &str) -> bool {
        if !self.room.can_follow(topics::BRIDGE) {
            return false;
        }
        let id = {
            let mut next = lock(&self.next);
            *next += 1;
            format!("pair-{}", *next)
        };
        let (tx, rx) = sync_channel(1);
        lock(&self.waiting).insert(id.clone(), tx);
        self.room.publish(
            topics::BRIDGE,
            topics::BRIDGE_PAIR_REQUEST,
            &json!({"request": id, "name": name, "platform": platform, "address": address}),
        );
        let allowed = rx.recv_timeout(self.wait).unwrap_or(false);
        lock(&self.waiting).remove(&id);
        allowed
    }

    pub fn answer(&self, id: &str, allow: bool) -> bool {
        match lock(&self.waiting).remove(id) {
            Some(tx) => tx.try_send(allow).is_ok(),
            None => false,
        }
    }

    pub fn refuse_all(&self) {
        for (_, tx) in lock(&self.waiting).drain() {
            let _ = tx.try_send(false);
        }
    }
}

pub struct DaemonSources {
    pub sessions: Arc<Mutex<Value>>,
    pub acting: Arc<ActingOps>,
    pub approvals: Arc<Approvals>,
    pub projects_dir: PathBuf,
}

fn agents_of(list: &Value) -> Vec<Agent> {
    serde_json::from_value(list.clone()).unwrap_or_default()
}

impl Sources for DaemonSources {
    fn claude_sessions(&self) -> Vec<Session> {
        let rows = agents_of(&lock(&self.sessions));
        let mut sessions = agents::sessions(&rows);
        let mut prompts = self.acting.prompts().list();
        prompts.sort_by_key(|p| p.created_ms);
        for prompt in prompts {
            if let Some(session) = sessions
                .iter_mut()
                .find(|s| s.id == prompt.session_id && s.pending.is_none())
            {
                session.pending = Some(pending(&prompt));
            }
        }
        sessions
    }

    fn projects_dir(&self) -> Option<PathBuf> {
        Some(self.projects_dir.clone())
    }

    fn models(&self) -> Vec<ModelChoice> {
        self.acting
            .catalog()
            .models(Tool::Claude)
            .into_iter()
            .filter(|m| m.tier == ModelTier::Current)
            .map(|m| ModelChoice {
                id: m.id,
                name: m.name,
            })
            .collect()
    }

    fn model_allowed(&self, id: &str) -> bool {
        self.acting.catalog().is_allowed(Tool::Claude, id)
    }

    fn context_window(&self, model: &str) -> Option<u64> {
        self.acting.catalog().context_window(Tool::Claude, model)
    }

    fn request_approval(&self, device_name: &str, platform: &str, address: &str) -> bool {
        self.approvals.request(device_name, platform, address)
    }

    fn now_ms(&self) -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0)
    }
}

pub fn pending(prompt: &Prompt) -> PendingPrompt {
    let kind = match (prompt.kind, &prompt.question) {
        (Kind::Question, Some(question)) => PromptKind::Question(
            question
                .items
                .iter()
                .map(|i| PromptQuestion {
                    header: i.header.clone(),
                    question: i.question.clone(),
                    options: i.options.clone(),
                    multi_select: i.multi_select,
                })
                .collect(),
        ),
        _ => PromptKind::Permission,
    };
    PendingPrompt {
        id: prompt.id.clone(),
        session_id: prompt.session_id.clone(),
        kind,
        tool_name: prompt
            .event
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        summary: prompt.summary(),
        created_ms: prompt.created_ms as f64,
    }
}

pub struct DaemonActing {
    pub ops: Arc<ActingOps>,
    pub sessions: Arc<Mutex<Value>>,
}

fn bridge_caller() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

fn failure(e: ApiError) -> BridgeFailure {
    match e.code {
        ErrorCode::Internal => BridgeFailure::internal(e.message),
        ErrorCode::Failed => BridgeFailure::not_reachable(e.message),
        _ => BridgeFailure::invalid(e.message),
    }
}

impl DaemonActing {
    fn call(&self, op: &str, args: Value) -> Result<Value, BridgeFailure> {
        self.ops.call(&bridge_caller(), op, &args).map_err(failure)
    }

    fn row_of(&self, session: &Session) -> Result<String, BridgeFailure> {
        let Control::Job(job) = &session.control else {
            return Err(BridgeFailure::view_only("Session này chỉ xem được."));
        };
        agents_of(&lock(&self.sessions))
            .into_iter()
            .find(|a| matches!(&a.target, AgentTarget::Job(j) if j == job))
            .map(|a| a.id)
            .ok_or_else(|| BridgeFailure::not_found("Không tìm thấy session."))
    }
}

impl Acting for DaemonActing {
    fn is_on(&self) -> bool {
        self.ops.is_on()
    }

    fn send_message(&self, text: &str, session: &Session) -> Result<(), BridgeFailure> {
        let id = self.row_of(session)?;
        self.call(ops::SESSION_REPLY, json!({"id": id, "text": text}))
            .map(|_| ())
    }

    fn send_now(&self, session: &Session) -> Result<(), BridgeFailure> {
        let id = self.row_of(session)?;
        self.call(ops::SESSION_INTERRUPT, json!({"id": id}))
            .map(|_| ())
    }

    fn respond(
        &self,
        prompt: &PendingPrompt,
        response: &PromptResponse,
    ) -> Result<(), BridgeFailure> {
        let (op, args) = match (response, &prompt.kind) {
            (PromptResponse::Allow, _) => (
                ops::SESSION_PERMISSION,
                json!({"id": prompt.id, "decision": "allow"}),
            ),
            (PromptResponse::Deny, _) => (
                ops::SESSION_PERMISSION,
                json!({"id": prompt.id, "decision": "deny"}),
            ),
            (PromptResponse::Answers(answers), PromptKind::Question(items)) => (
                ops::SESSION_ANSWER,
                json!({"id": prompt.id, "answers": answer_json(answers, items)}),
            ),
            (PromptResponse::Answers(_), PromptKind::Permission) => {
                return Err(BridgeFailure::invalid(
                    "Đây là yêu cầu cấp quyền, không phải câu hỏi.",
                ))
            }
        };
        self.call(op, args).map(|_| ())
    }

    fn new_session(&self, cwd: &Path, model: Option<&str>) -> Result<(), BridgeFailure> {
        let mut args = json!({"folder": cwd.to_string_lossy()});
        if let Some(full) = model.and_then(|short| model_id(short, self.ops.catalog())) {
            args["flags"] = json!(["--model", full]);
        }
        self.call(ops::SESSION_NEW, args).map(|_| ())
    }

    fn new_session_with_model(&self, cwd: &Path, model_id: &str) -> Result<(), BridgeFailure> {
        let args = json!({"folder": cwd.to_string_lossy(), "flags": ["--model", model_id]});
        self.call(ops::SESSION_NEW, args).map(|_| ())
    }

    fn take_over(&self, _: &Session, _: bool) -> Result<TakeoverOutcome, BridgeFailure> {
        Err(BridgeFailure::invalid(
            "Máy này chưa tiếp quản được session từ điện thoại.",
        ))
    }
}

pub(crate) fn model_id(short: &str, catalog: &CatalogService) -> Option<String> {
    catalog
        .models(Tool::Claude)
        .into_iter()
        .find(|m| m.tier == ModelTier::Current && m.id.split('-').nth(1) == Some(short))
        .map(|m| m.id)
}

fn answer_json(answers: &[String], items: &[PromptQuestion]) -> Value {
    Value::Array(
        answers
            .iter()
            .zip(items)
            .map(
                |(answer, item)| match item.options.iter().position(|o| o == answer) {
                    Some(i) => json!([i + 1]),
                    None => json!(answer),
                },
            )
            .collect(),
    )
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_become_option_numbers_or_text() {
        let item = PromptQuestion {
            header: "Pick".into(),
            question: "Which?".into(),
            options: vec!["Red".into(), "Blue".into()],
            multi_select: false,
        };
        let out = answer_json(&["Blue".into(), "green".into()], &[item.clone(), item]);
        assert_eq!(out, json!([[2], "green"]));
    }
}
