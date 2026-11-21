use crate::layout::Layout;
use notchling_local_api::{ApiError, Client, ClientError, ClientKind, ErrorCode};
use serde_json::{json, Map, Value};
use std::io::Write;
use std::path::Path;

pub const OK: i32 = 0;
pub const FAILED: i32 = 1;
pub const BAD_REQUEST: i32 = 2;
pub const NOT_RUNNING: i32 = 3;
pub const REFUSED: i32 = 4;
pub const DENIED: i32 = 5;
pub const PENDING: i32 = 6;
pub const BUSY: i32 = 7;
pub const EXPIRED: i32 = 124;
pub const STOPPED: i32 = 125;

pub const EXIT_CODES: &[(i32, &str, &str)] = &[
    (OK, "ok", "Done: applied, answered, allowed and applied, or read."),
    (
        FAILED,
        "failed",
        "Notchling tried and it didn't work, or the request was withdrawn.",
    ),
    (
        BAD_REQUEST,
        "bad_request",
        "The command line or a value is wrong; nothing changed.",
    ),
    (
        NOT_RUNNING,
        "not_running",
        "No daemon, no answer, a socket Notchling doesn't trust, or a daemon older than this command.",
    ),
    (
        REFUSED,
        "refused",
        "Not allowed: a switch is off, user-only, another session's, another organization's.",
    ),
    (
        DENIED,
        "denied",
        "The person said no, dismissed or cancelled.",
    ),
    (
        PENDING,
        "pending",
        "The request is saved and still waits; its id is in the output.",
    ),
    (BUSY, "busy", "A limit was reached."),
    (
        EXPIRED,
        "expired",
        "No answer in time; the request is closed.",
    ),
    (
        STOPPED,
        "stopped",
        "secret only: Notchling stopped before running the command.",
    ),
];

pub const REASONS: &[&str] = &[
    "usage",
    "bad_value",
    "unknown_key",
    "key_reused",
    "not_running",
    "untrusted_daemon",
    "old_daemon",
    "timeout",
    "switch_off",
    "app_only",
    "no_app",
    "not_yet",
    "never",
    "internal_op",
    "other_session",
    "other_organization",
    "no_session",
    "needs_consent",
    "busy",
    "denied",
    "cancelled",
    "expired",
    "withdrawn",
    "already_answered",
    "failed",
    "internal",
    "other",
];

pub const OLD_DAEMON: &str = "Notchling was updated. Open it to finish, then try again.";
pub const NOT_OPEN: &str = "Notchling isn't running. Ask the user to open it, then try again.";
pub const USAGE_MESSAGE: &str =
    "The command line is wrong. Run `notchling help --json` for every command.";

pub fn name_of(exit: i32) -> &'static str {
    EXIT_CODES
        .iter()
        .find(|(code, _, _)| *code == exit)
        .map_or("failed", |(_, name, _)| name)
}

pub fn exit_of_reason(reason: &str) -> Option<i32> {
    Some(match reason {
        "usage" | "bad_value" | "unknown_key" | "key_reused" => BAD_REQUEST,
        "not_running" | "untrusted_daemon" | "old_daemon" | "timeout" => NOT_RUNNING,
        "switch_off" | "app_only" | "no_app" | "not_yet" | "never" | "internal_op"
        | "other_session" | "other_organization" | "no_session" | "needs_consent" | "other" => {
            REFUSED
        }
        "busy" => BUSY,
        "denied" | "cancelled" => DENIED,
        "expired" => EXPIRED,
        "withdrawn" | "already_answered" | "failed" | "internal" => FAILED,
        _ => return None,
    })
}

pub fn wants_json(args: &[String]) -> bool {
    args.iter()
        .take_while(|a| a.as_str() != "--")
        .any(|a| a == "--json")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub exit: i32,
    pub code: String,
    pub reason: String,
    pub message: String,
    pub human: Option<String>,
    pub op: Option<String>,
    pub switch: Option<String>,
    pub ask: Option<Value>,
    pub retry_after_ms: Option<u64>,
    pub request: Option<Value>,
    pub never: bool,
    pub phase: Option<u64>,
}

fn code_for(exit: i32, wire: &str) -> String {
    match exit {
        NOT_RUNNING => "not_running",
        DENIED => "denied",
        PENDING => "pending",
        BUSY => "busy",
        EXPIRED => "expired",
        STOPPED => "stopped",
        _ => wire,
    }
    .to_string()
}

impl Failure {
    pub fn new(exit: i32, reason: &str, message: impl Into<String>) -> Failure {
        Failure {
            exit,
            code: code_for(
                exit,
                match exit {
                    BAD_REQUEST => "usage",
                    REFUSED => "forbidden",
                    _ => "failed",
                },
            ),
            reason: reason.to_string(),
            message: message.into(),
            human: None,
            op: None,
            switch: None,
            ask: None,
            retry_after_ms: None,
            request: None,
            never: false,
            phase: None,
        }
    }

    pub fn usage(human: impl Into<String>) -> Failure {
        let mut failure = Failure::new(BAD_REQUEST, "usage", USAGE_MESSAGE);
        failure.human = Some(human.into());
        failure
    }

    pub fn not_running() -> Failure {
        Failure::new(NOT_RUNNING, "not_running", NOT_OPEN)
    }

    pub fn failed(message: impl Into<String>) -> Failure {
        Failure::new(FAILED, "failed", message)
    }

    pub fn with_human(mut self, human: impl Into<String>) -> Failure {
        self.human = Some(human.into());
        self
    }

    pub fn with_op(mut self, op: &str) -> Failure {
        self.op = Some(op.to_string());
        self
    }

    pub fn from_api(op: Option<&str>, e: ApiError) -> Failure {
        if e.code == ErrorCode::UnknownOp {
            let mut failure = Failure::new(NOT_RUNNING, "old_daemon", OLD_DAEMON);
            failure.op = op.map(str::to_string);
            return failure;
        }
        let data = e.data.as_ref().and_then(Value::as_object);
        let field = |name: &str| data.and_then(|d| d.get(name));
        let given = field("reason")
            .and_then(Value::as_str)
            .filter(|r| REASONS.contains(r));
        let (exit, reason) = match (given, e.code) {
            (Some(reason), _) => (exit_of_reason(reason).unwrap_or(FAILED), reason),
            (None, ErrorCode::BadRequest) => (BAD_REQUEST, "bad_value"),
            (None, ErrorCode::Forbidden) => (REFUSED, "other"),
            (None, ErrorCode::NeedsApproval) => (REFUSED, "no_app"),
            (None, ErrorCode::Failed) => (FAILED, "failed"),
            (None, _) => (FAILED, "internal"),
        };
        let mut failure = Failure::new(exit, reason, e.message.clone());
        failure.code = code_for(exit, e.code.as_str());
        failure.op = op.map(str::to_string);
        failure.switch = field("switch").and_then(Value::as_str).map(str::to_string);
        failure.ask = field("ask").filter(|v| v.is_object()).cloned();
        failure.retry_after_ms = field("retryAfterMs").and_then(Value::as_u64);
        failure.request = field("request").filter(|v| !v.is_null()).cloned();
        failure.never = field("never").and_then(Value::as_bool).unwrap_or(false);
        failure.phase = field("phase").and_then(Value::as_u64);
        failure
    }

    pub fn from_client(op: Option<&str>, e: ClientError) -> Failure {
        let mut failure = match e {
            ClientError::NotRunning(_) | ClientError::Unsupported | ClientError::Closed => {
                Failure::not_running()
            }
            ClientError::Timeout => Failure::new(
                NOT_RUNNING,
                "timeout",
                "Notchling didn't answer in time. Ask the user to open it, then try again.",
            ),
            ClientError::UntrustedServer(_) => Failure::new(
                NOT_RUNNING,
                "untrusted_daemon",
                "The socket isn't served by a trusted Notchling daemon.",
            ),
            ClientError::Api(api) => return Failure::from_api(op, api),
            ClientError::Protocol(_) => {
                Failure::new(FAILED, "internal", "Notchling sent something unexpected.")
            }
            ClientError::Io(_) => Failure::new(FAILED, "internal", "Notchling couldn't be reached."),
        };
        failure.op = op.map(str::to_string);
        failure
    }

    pub fn to_value(&self) -> Value {
        let mut body = Map::new();
        body.insert("exit".into(), json!(self.exit));
        body.insert("code".into(), json!(self.code));
        body.insert("reason".into(), json!(self.reason));
        body.insert("message".into(), json!(self.message));
        body.insert("op".into(), json!(self.op));
        body.insert("switch".into(), json!(self.switch));
        body.insert("ask".into(), json!(self.ask));
        body.insert("retryAfterMs".into(), json!(self.retry_after_ms));
        body.insert("request".into(), json!(self.request));
        body.insert("never".into(), json!(self.never));
        body.insert("phase".into(), json!(self.phase));
        json!({ "error": Value::Object(body) })
    }

    pub fn line(&self) -> String {
        let text = self.human.as_deref().unwrap_or(&self.message);
        format!("{text} ({})", name_of(self.exit))
    }

    pub fn emit(&self, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
        if json {
            let _ = writeln!(out, "{}", self.to_value());
        } else {
            let _ = writeln!(err, "{}", self.line());
        }
        self.exit
    }

    pub fn report(&self, json: bool) -> i32 {
        self.emit(json, &mut std::io::stdout(), &mut std::io::stderr())
    }
}

pub fn connect(socket: Option<&Path>) -> Result<Client, Failure> {
    let layout = Layout::from_env(socket).ok_or_else(Failure::not_running)?;
    Client::connect(&layout.socket, ClientKind::Cli, env!("CARGO_PKG_VERSION"))
        .map_err(|e| Failure::from_client(None, e))
}

pub fn call(
    client: &mut Client,
    op: &str,
    args: Value,
) -> Result<Value, Failure> {
    client
        .call(op, args)
        .map_err(|e| Failure::from_client(Some(op), e))
}

pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
#[path = "contract_tests.rs"]
mod tests;
