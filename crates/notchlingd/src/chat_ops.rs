use crate::acting_ops::Room;
use crate::op_group::OpGroup;
use notchling_local_api::{ops, topics, ApiError, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

const USAGE: &str = "chat.open_file takes {\"path\": \"/absolute/path\", \"session\": text}";
const NOT_A_FILE: &str = "That path is not a regular file that exists.";
const THIS_MAC_ONLY: &str =
    "chat.open_file shows a file in this Mac's Chat window, so another machine can't ask for it.";

pub struct ChatOps {
    room: Arc<dyn Room>,
}

impl ChatOps {
    pub fn new(room: Arc<dyn Room>) -> ChatOps {
        ChatOps { room }
    }

    fn open_file(&self, args: &Value) -> Result<Value, ApiError> {
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .filter(|p| Path::new(p).is_absolute())
            .ok_or_else(|| ApiError::bad_request(USAGE))?;
        let session = match args.get("session") {
            None | Some(Value::Null) => None,
            Some(Value::String(id)) if !id.is_empty() => Some(id.as_str()),
            Some(_) => return Err(ApiError::bad_request(USAGE)),
        };
        let shown = std::fs::canonicalize(path)
            .ok()
            .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file()))
            .ok_or_else(|| ApiError::bad_request(NOT_A_FILE))?;
        self.room.publish(
            topics::CHAT,
            topics::CHAT_OPEN_FILE,
            &json!({"path": shown, "session": session}),
        );
        Ok(json!({"ok": true, "path": shown}))
    }
}

impl OpGroup for ChatOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::CHAT_OPEN_FILE).then_some(OpSpec::AGENT_ACTS)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if caller.client == ClientKind::Node {
            return Err(ApiError::new(ErrorCode::Forbidden, THIS_MAC_ONLY));
        }
        match op {
            ops::CHAT_OPEN_FILE => self.open_file(args),
            _ => Err(ApiError::bad_request(USAGE)),
        }
    }

    fn topics(&self) -> &[&'static str] {
        &[topics::CHAT]
    }

    fn topic_tier(&self, topic: &str) -> Option<Tier> {
        (topic == topics::CHAT).then_some(Tier::App)
    }
}
