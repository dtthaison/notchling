use super::connection::Connection;
use super::inbox::InboxOutcome;
use super::model::{
    BridgeViz, ChatMessage, ChatPage, Command, ModelChoice, PendingPrompt, PhoneOptions,
    PromptResponse, Session, TakeoverOutcome,
};
use super::wire::BridgeFailure;
use crate::inbox::{ActionId, Item};
use std::path::Path;
use std::sync::Arc;

pub trait BridgeBackend: Send + Sync {
    fn sessions(&self) -> Vec<Session>;

    fn chat_messages(
        &self,
        session_id: &str,
        subagent_id: Option<&str>,
    ) -> Result<Vec<ChatMessage>, BridgeFailure>;

    fn chat_page(
        &self,
        session_id: &str,
        subagent_id: Option<&str>,
        before: Option<&str>,
        after: Option<&str>,
        limit: usize,
    ) -> Result<ChatPage, BridgeFailure> {
        let all = self.chat_messages(session_id, subagent_id)?;
        ChatPage::page(&all, before, after, limit)
    }

    fn chat_image(
        &self,
        _session_id: &str,
        _image_id: &str,
        _max_width: i64,
    ) -> Result<Vec<u8>, BridgeFailure> {
        Err(BridgeFailure::not_found("Không thấy ảnh này."))
    }

    fn commands(&self, _session_id: &str) -> Vec<Command> {
        Vec::new()
    }

    fn viz(&self, session_id: &str, include_entity: bool) -> Result<BridgeViz, BridgeFailure>;

    fn web_snapshot(&self, url: &str, width: i64) -> Result<Vec<u8>, BridgeFailure>;

    fn send_message(&self, text: &str, session: &Session) -> Result<(), BridgeFailure>;

    fn acting_on(&self) -> bool {
        true
    }

    fn send_now(&self, _session: &Session) -> Result<(), BridgeFailure> {
        Err(BridgeFailure::invalid(
            "Backend này không ngắt được lượt đang chạy.",
        ))
    }

    fn pending_prompt(&self, request_id: &str) -> Option<PendingPrompt>;

    fn respond(
        &self,
        prompt: &PendingPrompt,
        response: &PromptResponse,
    ) -> Result<(), BridgeFailure>;

    fn new_session(&self, cwd: &Path, model: Option<&str>) -> Result<(), BridgeFailure>;

    fn models(&self) -> Vec<ModelChoice> {
        Vec::new()
    }

    fn model_allowed(&self, _id: &str) -> bool {
        false
    }

    fn new_session_with_model(&self, _cwd: &Path, _model_id: &str) -> Result<(), BridgeFailure> {
        Err(BridgeFailure::invalid(
            "Backend này không chọn được model theo id.",
        ))
    }

    fn take_over(
        &self,
        session: &Session,
        stop_original: bool,
    ) -> Result<TakeoverOutcome, BridgeFailure>;

    fn recent_projects(&self) -> Vec<String>;

    fn request_approval(&self, device_name: &str, platform: &str, address: &str) -> bool;

    fn start_phone(
        &self,
        connection: &Arc<Connection>,
        options: &PhoneOptions,
    ) -> Result<(), BridgeFailure>;

    fn stop_phone(&self, connection: &Arc<Connection>);

    fn inbox_items(&self) -> Vec<Item> {
        Vec::new()
    }

    fn inbox_answer(
        &self,
        _item_id: &str,
        _action: ActionId,
    ) -> Result<InboxOutcome, BridgeFailure> {
        Err(BridgeFailure::not_found("Không có hộp thư này."))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
}

pub trait Devices: Send + Sync {
    fn authenticate(&self, token: &str) -> Option<PairedDevice>;
    fn touch(&self, id: &str);
    fn register(&self, name: &str, platform: &str) -> Result<(PairedDevice, String), String>;
}

pub trait PairingCodes: Send + Sync {
    fn consume(&self, code: &str, now_ms: f64) -> bool;
}

pub trait RateLimiter: Send + Sync {
    fn is_blocked(&self, address: &str, now_ms: f64) -> bool;
    fn record_failure(&self, address: &str, now_ms: f64);
    fn record_success(&self, address: &str);
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuditEntry {
    pub device: String,
    pub device_id: String,
    pub op: String,
    pub session: Option<String>,
    pub length: Option<usize>,
    pub address: Option<String>,
    pub stopped_pid: Option<i32>,
}

pub trait Audit: Send + Sync {
    fn append(&self, entry: AuditEntry);
}
