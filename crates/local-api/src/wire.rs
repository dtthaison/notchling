//! What goes over the socket: one JSON object per line.
//!
//! - Request `{"id": <u64>, "op": "<name>", "args": {...}}`.
//! - Reply `{"id": <u64>, "ok": <result>}` or `{"id": <u64>, "err": {"code": "<code>", "message": "..."}}`.
//!   A success may carry more fields beside `ok` (`sessions.list`: `"partial": true` when it
//!   answered before the daemon's first read of the list); a client ignores ones it doesn't know.
//! - Event, only after `events.subscribe`: `{"event": "<name>", "data": {...}}`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;

/// The API version this crate speaks; `hello` must name it.
pub const API_VERSION: u32 = 1;

/// The op names: Milestone 2's reads, then Milestone 3's acting ops (app tier, refused while the
/// daemon's `acting` setting is off), their reads, and the hook's.
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

    /// Milestone 5's Codex ops (E6a), app tier and refused while acting is off:
    /// `codex.send {id | draft_folder, text, model?, effort?}`, `codex.stop {id}`,
    /// `codex.delete {id}`.
    pub const CODEX_SEND: &str = "codex.send";
    pub const CODEX_STOP: &str = "codex.stop";
    pub const CODEX_DELETE: &str = "codex.delete";

    /// Milestone 4's phone bridge, app tier. The bridge is off at every start; `bridge.set
    /// {"on": bool}` and `bridge.status` answer `{state, port, fingerprint, ...}`.
    pub const BRIDGE_SET: &str = "bridge.set";
    pub const BRIDGE_STATUS: &str = "bridge.status";
    /// `{code, payload, expires_at, hosts}`: a one-time code and the QR's text.
    pub const BRIDGE_PAIR_BEGIN: &str = "bridge.pair.begin";
    /// `{"request": id, "allow": bool}`: the answer to a `bridge.pair.request` event.
    pub const BRIDGE_PAIR_ANSWER: &str = "bridge.pair.answer";
    /// `{devices: [{id, name, platform, paired_at, last_seen}]}`.
    pub const BRIDGE_DEVICES: &str = "bridge.devices";
    /// `{"device": id}` → `{revoked: bool}`.
    pub const BRIDGE_REVOKE: &str = "bridge.revoke";
    /// `{enabled, configured, connected, last_connection, listening, hostname}`.
    pub const BRIDGE_TUNNEL_STATUS: &str = "bridge.tunnel.status";

    /// Milestone 5's pages library and session index, agent tier, reads that work whatever the
    /// `acting` switch says. The shapes are in `notchlingd`'s `pages_ops` module docs.
    /// `{text?, limit?}` → `{pages: [Page], sessions: [Session], indexing}`.
    pub const SEARCH_QUERY: &str = "search.query";
    /// `{}` → `{pages: [Page], truncated}`.
    pub const PAGES_LIST: &str = "pages.list";
    /// `{query}` → `{pages: [Page + describe], truncated}`.
    pub const PAGES_FIND: &str = "pages.find";
}

/// Event topics and the events they carry.
pub mod topics {
    /// `events.subscribe {"topics": ["sessions"]}`.
    pub const SESSIONS: &str = "sessions";
    /// The full new session list, as `sessions.list` returns it.
    pub const SESSIONS_CHANGED: &str = "sessions.changed";
    /// Questions and permissions a hook holds open, so the app (and later the phone) share one
    /// answered state.
    pub const PROMPTS: &str = "prompts";
    pub const PROMPT_PENDING: &str = "prompt.pending";
    pub const PROMPT_RESOLVED: &str = "prompt.resolved";
    /// The daemon's own settings.
    pub const SETTINGS: &str = "settings";
    /// `{"acting": bool}`, after `settings.set_acting`.
    pub const ACTING_CHANGED: &str = "settings.acting";
    /// Other tools' sessions (Milestone 5): Codex turns and the rows they change.
    pub const TOOLS: &str = "tools";
    /// `{"id": "codex:<session>" | null, "session", "state": "started" | "queued" | "ended" |
    /// "failed", "waiting"?, "error"?}`.
    pub const CODEX_TURN: &str = "codex.turn";
    /// `{"id", "session", "row": <a sessions.list row> | null, "deleted"?: true}`: a session
    /// another tool runs changed (its row is given as `sessions.list` would, or `null` when it is
    /// gone).
    pub const SESSION_CHANGED: &str = "session.changed";
    /// The phone bridge (app tier).
    pub const BRIDGE: &str = "bridge";
    /// `{"request", "name", "platform", "address"}`: a phone gave the right code and waits for the person at
    /// the computer (`bridge.pair.answer`). Unanswered, it is refused.
    pub const BRIDGE_PAIR_REQUEST: &str = "bridge.pair.request";
    /// The status after a change (`bridge.status`'s answer).
    pub const BRIDGE_CHANGED: &str = "bridge.changed";
}

/// Who the caller says it is. Self-reported, so it never decides the tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    App,
    Cli,
    Hook,
}

/// What a caller may do (decision 0007). Decided by the daemon from the peer's executable path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// The installed desktop app: may do everything.
    App,
    /// Everything else, the command line agents run included: only its narrow operations.
    Agent,
}

/// Why a call failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    UnknownOp,
    Forbidden,
    NeedsApproval,
    Internal,
    /// The op exists and was allowed, but this daemon doesn't hold its code yet.
    Unimplemented,
    /// The op ran and the session refused it (a dialog open, a question gone); the message says why.
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

/// An `err` reply's body.
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

/// A request line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub op: String,
    #[serde(default)]
    pub args: Value,
}

impl Request {
    /// Reads a request line. On failure the error carries the line's `id` when it had one, so the
    /// reply can still name it (0 otherwise).
    pub fn parse(line: &[u8]) -> Result<Request, (u64, ApiError)> {
        let value: Value = serde_json::from_slice(line)
            .map_err(|e| (0, ApiError::bad_request(format!("not JSON: {e}"))))?;
        let id = value.get("id").and_then(Value::as_u64).unwrap_or(0);
        serde_json::from_value(value)
            .map_err(|e| (id, ApiError::bad_request(format!("not a request: {e}"))))
    }
}

/// `hello`'s arguments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub client: ClientKind,
    pub version: String,
    pub api: u32,
}

/// `hello`'s result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloReply {
    pub api: u32,
    pub daemon: String,
    pub tier: Tier,
}

/// How many clients of each kind have said hello.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCounts {
    pub app: u32,
    pub cli: u32,
    pub hook: u32,
}

/// `daemon.status`'s result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatus {
    pub version: String,
    pub pid: u32,
    pub uptime_secs: u64,
    pub clients: ClientCounts,
    /// Whether the daemon may act on sessions (Milestone 3's switch, off by default); left out by
    /// a handler that has no switch ([`crate::Handler::acting`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acting: Option<bool>,
}

/// `events.subscribe`'s arguments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscribe {
    pub topics: Vec<String>,
}

/// An event line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub event: String,
    #[serde(default)]
    pub data: Value,
}

/// A line the client reads: a reply to one of its requests, or an event.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    Reply {
        id: u64,
        result: Result<Value, ApiError>,
    },
    Event(Event),
}

impl Incoming {
    /// Reads a line from the daemon. A reply whose `ok` is `null` is still a success.
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

/// A success reply.
pub fn ok_reply(id: u64, result: Value) -> Value {
    json!({ "id": id, "ok": result })
}

/// A failure reply.
pub fn err_reply(id: u64, error: &ApiError) -> Value {
    json!({ "id": id, "err": { "code": error.code.as_str(), "message": error.message } })
}

/// An event line's object.
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
