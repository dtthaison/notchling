//! What the router needs from outside: the app's sessions and actions ([`BridgeBackend`], the
//! Swift `BridgeBackend` protocol that `PhoneModel` and `LiveBridgeBackend` answered), and the
//! bridge's own security state ([`Devices`], [`PairingCodes`], [`RateLimiter`], [`Audit`]), which
//! the transport (E5b) keeps on disk in the Swift app's formats.
//!
//! Every call blocks. The router calls them on the thread that handles the frame.

use super::connection::Connection;
use super::model::{
    BridgeViz, ChatMessage, ChatPage, Command, ModelChoice, PendingPrompt, PhoneOptions,
    PromptResponse, Session, TakeoverOutcome,
};
use super::wire::BridgeFailure;
use std::path::Path;
use std::sync::Arc;

/// Sessions, chats and the actions the phone may ask for.
///
/// The acting calls (`send_message`, `send_now`, `respond`, `new_session`, `take_over`) are where
/// the acting switch applies: a backend refuses them while acting is off. [`super::live`]'s backend
/// sends them to an [`super::live::Acting`] the daemon implements on its acting ops.
pub trait BridgeBackend: Send + Sync {
    fn sessions(&self) -> Vec<Session>;

    fn chat_messages(
        &self,
        session_id: &str,
        subagent_id: Option<&str>,
    ) -> Result<Vec<ChatMessage>, BridgeFailure>;

    /// One page of history. The default pages [`Self::chat_messages`] in memory.
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

    /// A JPEG of a picture the session's chat names, at most `max_width` wide.
    fn chat_image(
        &self,
        _session_id: &str,
        _image_id: &str,
        _max_width: i64,
    ) -> Result<Vec<u8>, BridgeFailure> {
        Err(BridgeFailure::not_found("Không thấy ảnh này."))
    }

    /// The slash commands the session can run.
    fn commands(&self, _session_id: &str) -> Vec<Command> {
        Vec::new()
    }

    fn viz(&self, session_id: &str, include_entity: bool) -> Result<BridgeViz, BridgeFailure>;

    /// A JPEG of a local page (`url` already checked to be http(s) on localhost).
    fn web_snapshot(&self, url: &str, width: i64) -> Result<Vec<u8>, BridgeFailure>;

    fn send_message(&self, text: &str, session: &Session) -> Result<(), BridgeFailure>;

    /// Whether the acting calls would run now. Asked before work that only leads to one (a photo
    /// upload), so nothing is held for a call that will be refused.
    fn acting_on(&self) -> bool {
        true
    }

    /// One Esc to an owned terminal, so Claude Code runs the queued message next.
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

    /// `cwd` is already checked: an existing folder inside home, symlinks resolved.
    fn new_session(&self, cwd: &Path, model: Option<&str>) -> Result<(), BridgeFailure>;

    /// The Claude models a new session may start on now (current ones), for `sessions.list`. A
    /// backend with no catalog lists none, and the phone keeps its short names.
    fn models(&self) -> Vec<ModelChoice> {
        Vec::new()
    }

    /// Whether `id` is a model a session may start on, by the same check the start itself makes.
    fn model_allowed(&self, _id: &str) -> bool {
        false
    }

    /// Like `new_session`, on a model id `model_allowed` accepted.
    fn new_session_with_model(&self, _cwd: &Path, _model_id: &str) -> Result<(), BridgeFailure> {
        Err(BridgeFailure::invalid(
            "Backend này không chọn được model theo id.",
        ))
    }

    /// Resumes a view-only session in an owned pty (stopping the original first if asked).
    fn take_over(
        &self,
        session: &Session,
        stop_original: bool,
    ) -> Result<TakeoverOutcome, BridgeFailure>;

    fn recent_projects(&self) -> Vec<String>;

    /// Asks the person at the computer whether to pair this device, and waits for the answer.
    fn request_approval(&self, device_name: &str, platform: &str, address: &str) -> bool;

    fn start_phone(
        &self,
        connection: &Arc<Connection>,
        options: &PhoneOptions,
    ) -> Result<(), BridgeFailure>;

    fn stop_phone(&self, connection: &Arc<Connection>);
}

/// A paired phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
}

/// The paired devices and their tokens (Swift `DeviceStore`, `devices.json`).
pub trait Devices: Send + Sync {
    /// The device a token belongs to; tokens are compared in constant time.
    fn authenticate(&self, token: &str) -> Option<PairedDevice>;
    /// Records that the device was just seen.
    fn touch(&self, id: &str);
    /// Saves a new device and returns it with its token, shown once.
    fn register(&self, name: &str, platform: &str) -> Result<(PairedDevice, String), String>;
}

/// One-time pairing codes (Swift `PairingCodes`).
pub trait PairingCodes: Send + Sync {
    /// True once for a code that was issued and hasn't expired.
    fn consume(&self, code: &str, now_ms: f64) -> bool;
}

/// Locks an address out after repeated failed logins (Swift `AuthRateLimiter`).
pub trait RateLimiter: Send + Sync {
    fn is_blocked(&self, address: &str, now_ms: f64) -> bool;
    fn record_failure(&self, address: &str, now_ms: f64);
    fn record_success(&self, address: &str);
}

/// One executed remote action. Never the text sent, only its length.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuditEntry {
    pub device: String,
    pub op: String,
    pub session: Option<String>,
    pub length: Option<usize>,
    /// The client's address, only for tunnel clients.
    pub address: Option<String>,
    pub stopped_pid: Option<i32>,
}

/// The audit log (Swift `AuditLog`, `remote-log.jsonl`).
pub trait Audit: Send + Sync {
    fn append(&self, entry: AuditEntry);
}
