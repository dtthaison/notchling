use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;

pub const API_VERSION: u32 = 1;

pub mod ops {
    pub const HELLO: &str = "hello";
    pub const DAEMON_STATUS: &str = "daemon.status";
    pub const SESSIONS_LIST: &str = "sessions.list";
    pub const EVENTS_SUBSCRIBE: &str = "events.subscribe";

    pub const SESSION_REPLY: &str = "session.reply";
    pub const SESSION_ANSWER: &str = "session.answer";
    pub const SESSION_PERMISSION: &str = "session.permission";
    pub const SESSION_INTERRUPT: &str = "session.interrupt";
    pub const SESSION_STOP: &str = "session.stop";
    pub const SESSION_REMOVE: &str = "session.remove";
    pub const SESSION_TAKE_BACK: &str = "session.take_back";
    pub const SESSION_NEW: &str = "session.new";
    pub const FOLDER_TRUST: &str = "folder.trust";
    pub const SESSION_MOVE: &str = "session.move";
    pub const SESSION_COMMAND: &str = "session.command";
    pub const SETTINGS_SET_ACTING: &str = "settings.set_acting";
    pub const SETTINGS_GET_ACTING: &str = "settings.get_acting";
    pub const PROMPTS_LIST: &str = "prompts.list";
    pub const SESSION_QUESTION: &str = "session.question";
    pub const HOOK_REQUEST: &str = "hook.request";

    pub const CODEX_SEND: &str = "codex.send";
    pub const CODEX_STOP: &str = "codex.stop";
    pub const CODEX_DELETE: &str = "codex.delete";

    pub const AGY_SESSIONS: &str = "agy.sessions";
    pub const AGY_STATE: &str = "agy.state";
    pub const AGY_CONVERSATION: &str = "agy.conversation";
    pub const AGY_OPEN: &str = "agy.open";
    pub const AGY_SEND: &str = "agy.send";
    pub const AGY_CANCEL: &str = "agy.cancel";
    pub const AGY_SET_OPTION: &str = "agy.set_option";
    pub const AGY_PERMISSION: &str = "agy.permission";

    pub const BRIDGE_SET: &str = "bridge.set";
    pub const BRIDGE_STATUS: &str = "bridge.status";
    pub const BRIDGE_PAIR_BEGIN: &str = "bridge.pair.begin";
    pub const BRIDGE_PAIR_ANSWER: &str = "bridge.pair.answer";
    pub const BRIDGE_DEVICES: &str = "bridge.devices";
    pub const BRIDGE_REVOKE: &str = "bridge.revoke";
    pub const BRIDGE_TUNNEL_STATUS: &str = "bridge.tunnel.status";

    pub const SEARCH_QUERY: &str = "search.query";
    pub const PAGES_LIST: &str = "pages.list";
    pub const PAGES_FIND: &str = "pages.find";

    pub const INSTALL_PLAN: &str = "install.plan";
    pub const INSTALL_APPLY: &str = "install.apply";
    pub const INSTALL_REMOVE: &str = "install.remove";
    pub const SETTINGS_SET_SWITCH: &str = "settings.set_switch";
    pub const SETTINGS_SWITCHES: &str = "settings.switches";

    pub const ACCOUNTS_LIST: &str = "accounts.list";
    pub const ACCOUNTS_SWITCH: &str = "accounts.switch";
    pub const ACCOUNTS_ADD_BEGIN: &str = "accounts.add.begin";
    pub const ACCOUNTS_ADD_FINISH: &str = "accounts.add.finish";
    pub const ACCOUNTS_REMOVE: &str = "accounts.remove";
    pub const ACCOUNTS_MOVE_ALL: &str = "accounts.move_all";
    pub const ACCOUNTS_RESULT: &str = "accounts.result";
    pub const LANES_LIST: &str = "lanes.list";
    pub const LANES_CREATE: &str = "lanes.create";
    pub const LANES_REMOVE: &str = "lanes.remove";
    pub const LANES_SWITCH: &str = "lanes.switch";

    pub const PROVIDERS_LIST: &str = "providers.list";
    pub const PROVIDERS_SWITCH: &str = "providers.switch";
    pub const PROVIDERS_ADD_BEGIN: &str = "providers.add.begin";
    pub const PROVIDERS_ADD_CANCEL: &str = "providers.add.cancel";
    pub const PROVIDERS_REMOVE: &str = "providers.remove";
    pub const PROVIDERS_USAGE: &str = "providers.usage";

    pub const VAULT_FIND: &str = "vault.find";
    pub const VAULT_USE: &str = "vault.use";
    pub const VAULT_REQUEST: &str = "vault.request";
    pub const VAULT_LIST: &str = "vault.list";
    pub const VAULT_SAVE: &str = "vault.save";
    pub const VAULT_REMOVE: &str = "vault.remove";
    pub const VAULT_APPROVAL_ANSWER: &str = "vault.approval.answer";
    pub const VAULT_REQUEST_CANCEL: &str = "vault.request.cancel";
    pub const VAULT_PENDING: &str = "vault.pending";

    pub const CATALOG_GET: &str = "catalog.get";

    pub const INBOX_LIST: &str = "inbox.list";
    pub const INBOX_ANSWER: &str = "inbox.answer";
    pub const INBOX_SEEN: &str = "inbox.seen";

    pub const GIT_STATUS: &str = "git.status";
    pub const GIT_DIFF: &str = "git.diff";
}

pub mod topics {
    pub const SESSIONS: &str = "sessions";
    pub const SESSIONS_CHANGED: &str = "sessions.changed";
    pub const PROMPTS: &str = "prompts";
    pub const PROMPT_PENDING: &str = "prompt.pending";
    pub const PROMPT_RESOLVED: &str = "prompt.resolved";
    pub const SETTINGS: &str = "settings";
    pub const ACTING_CHANGED: &str = "settings.acting";
    pub const SWITCH_CHANGED: &str = "settings.switch";
    pub const ACCOUNTS: &str = "accounts";
    pub const ACCOUNTS_CHANGED: &str = "accounts.changed";
    pub const ACCOUNTS_ADD_ENDED: &str = "accounts.add.ended";
    pub const ACCOUNTS_DONE: &str = "accounts.done";
    pub const TOOLS: &str = "tools";
    pub const CATALOG_CHANGED: &str = "catalog.changed";
    pub const CODEX_TURN: &str = "codex.turn";
    pub const SESSION_CHANGED: &str = "session.changed";
    pub const AGY_PERMISSION_PENDING: &str = "agy.permission.pending";
    pub const AGY_PERMISSION_RESOLVED: &str = "agy.permission.resolved";
    pub const BRIDGE: &str = "bridge";
    pub const BRIDGE_PAIR_REQUEST: &str = "bridge.pair.request";
    pub const BRIDGE_CHANGED: &str = "bridge.changed";
    pub const PROVIDERS: &str = "providers";
    pub const PROVIDERS_CHANGED: &str = "providers.changed";

    pub const VAULT: &str = "vault";
    pub const VAULT_APPROVAL_PENDING: &str = "vault.approval.pending";
    pub const VAULT_APPROVAL_RESOLVED: &str = "vault.approval.resolved";
    pub const VAULT_REQUEST_PENDING: &str = "vault.request.pending";
    pub const VAULT_REQUEST_RESOLVED: &str = "vault.request.resolved";
    pub const VAULT_CHANGED: &str = "vault.changed";

    pub const INBOX: &str = "inbox";
    pub const INBOX_CHANGED: &str = "inbox.changed";
    pub const INBOX_REMOVED: &str = "inbox.removed";
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    App,
    Cli,
    Hook,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    App,
    Agent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    UnknownOp,
    Forbidden,
    NeedsApproval,
    Internal,
    Unimplemented,
    Failed,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::BadRequest => "bad_request",
            ErrorCode::UnknownOp => "unknown_op",
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::NeedsApproval => "needs_approval",
            ErrorCode::Internal => "internal",
            ErrorCode::Unimplemented => "unimplemented",
            ErrorCode::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> ApiError {
        ApiError {
            code,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorCode::BadRequest, message)
    }

    pub fn internal(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorCode::Internal, message)
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for ApiError {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub op: String,
    #[serde(default)]
    pub args: Value,
}

impl Request {
    pub fn parse(line: &[u8]) -> Result<Request, (u64, ApiError)> {
        let value: Value = serde_json::from_slice(line)
            .map_err(|e| (0, ApiError::bad_request(format!("not JSON: {e}"))))?;
        let id = value.get("id").and_then(Value::as_u64).unwrap_or(0);
        serde_json::from_value(value)
            .map_err(|e| (id, ApiError::bad_request(format!("not a request: {e}"))))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub client: ClientKind,
    pub version: String,
    pub api: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloReply {
    pub api: u32,
    pub daemon: String,
    pub tier: Tier,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCounts {
    pub app: u32,
    pub cli: u32,
    pub hook: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatus {
    pub version: String,
    pub pid: u32,
    pub uptime_secs: u64,
    pub clients: ClientCounts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acting: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscribe {
    pub topics: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub event: String,
    #[serde(default)]
    pub data: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    Reply {
        id: u64,
        result: Result<Value, ApiError>,
    },
    Event(Event),
}

impl Incoming {
    pub fn parse(line: &[u8]) -> Result<Incoming, String> {
        let value: Value = serde_json::from_slice(line).map_err(|e| format!("not JSON: {e}"))?;
        let Value::Object(mut map) = value else {
            return Err("not an object".into());
        };
        if let Some(name) = map.get("event").and_then(Value::as_str) {
            let event = name.to_string();
            let data = map.remove("data").unwrap_or(Value::Null);
            return Ok(Incoming::Event(Event { event, data }));
        }
        let id = map
            .get("id")
            .and_then(Value::as_u64)
            .ok_or("a reply without an id")?;
        if let Some(err) = map.remove("err") {
            let err: ApiError =
                serde_json::from_value(err).map_err(|e| format!("a bad error: {e}"))?;
            return Ok(Incoming::Reply {
                id,
                result: Err(err),
            });
        }
        match map.remove("ok") {
            Some(ok) => Ok(Incoming::Reply { id, result: Ok(ok) }),
            None => Err("a reply with neither ok nor err".into()),
        }
    }
}

pub fn ok_reply(id: u64, result: Value) -> Value {
    json!({ "id": id, "ok": result })
}

pub fn err_reply(id: u64, error: &ApiError) -> Value {
    json!({ "id": id, "err": { "code": error.code.as_str(), "message": error.message } })
}

pub fn event_line(event: &str, data: &Value) -> Value {
    json!({ "event": event, "data": data })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_reads_with_default_args() {
        let r = Request::parse(br#"{"id":7,"op":"sessions.list"}"#).unwrap();
        assert_eq!(r.id, 7);
        assert_eq!(r.op, "sessions.list");
        assert_eq!(r.args, Value::Null);
    }

    #[test]
    fn a_bad_request_keeps_its_id_when_it_has_one() {
        let (id, e) = Request::parse(br#"{"id":9,"op":5}"#).unwrap_err();
        assert_eq!(id, 9);
        assert_eq!(e.code, ErrorCode::BadRequest);
        let (id, _) = Request::parse(b"\xff\xfe garbage").unwrap_err();
        assert_eq!(id, 0);
    }

    #[test]
    fn replies_and_events_round_trip() {
        let ok = serde_json::to_vec(&ok_reply(3, json!(null))).unwrap();
        assert_eq!(
            Incoming::parse(&ok).unwrap(),
            Incoming::Reply {
                id: 3,
                result: Ok(Value::Null)
            }
        );
        let e = ApiError::new(ErrorCode::NeedsApproval, "no app attached");
        let err = serde_json::to_vec(&err_reply(4, &e)).unwrap();
        assert_eq!(
            Incoming::parse(&err).unwrap(),
            Incoming::Reply {
                id: 4,
                result: Err(e)
            }
        );
        let ev = serde_json::to_vec(&event_line("sessions.changed", &json!([1]))).unwrap();
        assert_eq!(
            Incoming::parse(&ev).unwrap(),
            Incoming::Event(Event {
                event: "sessions.changed".into(),
                data: json!([1])
            })
        );
    }

    #[test]
    fn error_codes_are_the_wire_names() {
        let text = serde_json::to_string(&ErrorCode::NeedsApproval).unwrap();
        assert_eq!(text, "\"needs_approval\"");
        for code in [
            ErrorCode::BadRequest,
            ErrorCode::UnknownOp,
            ErrorCode::Forbidden,
            ErrorCode::NeedsApproval,
            ErrorCode::Internal,
        ] {
            assert_eq!(
                serde_json::to_value(code).unwrap(),
                Value::String(code.as_str().into())
            );
        }
    }

    #[test]
    fn the_m3_codes_are_wire_names_too() {
        for (code, name) in [
            (ErrorCode::Unimplemented, "unimplemented"),
            (ErrorCode::Failed, "failed"),
        ] {
            assert_eq!(code.as_str(), name);
            assert_eq!(
                serde_json::to_value(code).unwrap(),
                Value::String(name.into())
            );
        }
        assert_eq!(ops::SETTINGS_SET_ACTING, "settings.set_acting");
        assert_eq!(ops::HOOK_REQUEST, "hook.request");
        assert_eq!(ops::INSTALL_PLAN, "install.plan");
        assert_eq!(ops::INSTALL_APPLY, "install.apply");
        assert_eq!(ops::INSTALL_REMOVE, "install.remove");
        assert_eq!(topics::PROMPT_PENDING, "prompt.pending");
        assert_eq!(topics::PROMPT_RESOLVED, "prompt.resolved");
    }

    #[test]
    fn hello_reads_the_plan_shape() {
        let h: Hello =
            serde_json::from_str(r#"{"client":"cli","version":"0.1.0","api":1}"#).unwrap();
        assert_eq!(h.client, ClientKind::Cli);
        let r = HelloReply {
            api: 1,
            daemon: "0.1.0".into(),
            tier: Tier::Agent,
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"api":1,"daemon":"0.1.0","tier":"agent"})
        );
    }
}
