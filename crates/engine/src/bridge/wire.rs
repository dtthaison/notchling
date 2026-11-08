//! The text and binary frames on the wire (ConsoleCore's `BridgeWire` and `PhoneFrame`).
//!
//! Text frames are JSON written the way the Swift bridge writes them: keys sorted, slashes not
//! escaped, and a whole number written without a fraction (`1793541465000`, not `1793541465000.0`),
//! because the generated types keep times as `f64` and phones read them as numbers either way.

use notchling_protocol::generated::ErrorCode;
use serde::Serialize;
use serde_json::{Map, Value};

/// `version` in the `hello` and `pair` answers.
pub const VERSION: i64 = 1;

/// A refused request: its code and a message for the person (Vietnamese, as the Swift app's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeFailure {
    pub code: ErrorCode,
    pub message: String,
}

impl BridgeFailure {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> BridgeFailure {
        BridgeFailure {
            code,
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> BridgeFailure {
        BridgeFailure::new(ErrorCode::Unauthorized, message)
    }

    pub fn not_found(message: impl Into<String>) -> BridgeFailure {
        BridgeFailure::new(ErrorCode::NotFound, message)
    }

    pub fn view_only(message: impl Into<String>) -> BridgeFailure {
        BridgeFailure::new(ErrorCode::ViewOnly, message)
    }

    pub fn invalid(message: impl Into<String>) -> BridgeFailure {
        BridgeFailure::new(ErrorCode::Invalid, message)
    }

    pub fn busy(message: impl Into<String>) -> BridgeFailure {
        BridgeFailure::new(ErrorCode::Busy, message)
    }

    pub fn not_reachable(message: impl Into<String>) -> BridgeFailure {
        BridgeFailure::new(ErrorCode::NotReachable, message)
    }

    pub fn internal(message: impl Into<String>) -> BridgeFailure {
        BridgeFailure::new(ErrorCode::Internal, message)
    }
}

impl std::fmt::Display for BridgeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", code_name(self.code), self.message)
    }
}

impl std::error::Error for BridgeFailure {}

/// The code as it travels (`not_found`, `view_only`...).
pub fn code_name(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Unauthorized => "unauthorized",
        ErrorCode::NotFound => "not_found",
        ErrorCode::ViewOnly => "view_only",
        ErrorCode::Invalid => "invalid",
        ErrorCode::Busy => "busy",
        ErrorCode::NotReachable => "not_reachable",
        // The Mac never sends a code it doesn't know.
        ErrorCode::Internal | ErrorCode::Unknown => "internal",
    }
}

/// `{"id", "ok": true, "data"}`. `id` is the request's, echoed whatever JSON it was; absent when the
/// request had none.
pub fn ok(id: Option<&Value>, data: Value) -> String {
    let mut object = Map::new();
    object.insert("ok".into(), Value::Bool(true));
    object.insert("data".into(), data);
    if let Some(id) = id {
        object.insert("id".into(), id.clone());
    }
    text(&Value::Object(object))
}

/// `{"id", "ok": false, "error": {"code", "message"}}`.
pub fn error(id: Option<&Value>, failure: &BridgeFailure) -> String {
    let mut detail = Map::new();
    detail.insert("code".into(), Value::from(code_name(failure.code)));
    detail.insert("message".into(), Value::from(failure.message.as_str()));
    let mut object = Map::new();
    object.insert("ok".into(), Value::Bool(false));
    object.insert("error".into(), Value::Object(detail));
    if let Some(id) = id {
        object.insert("id".into(), id.clone());
    }
    text(&Value::Object(object))
}

/// `{"event": name, ...fields}`.
pub fn event(name: &str, mut fields: Map<String, Value>) -> String {
    fields.insert("event".into(), Value::from(name));
    text(&Value::Object(fields))
}

/// Any serializable value as JSON (`null` if it can't be, which the generated types never are).
pub fn object<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// The request id as a binary frame's number: an integer id, truncated to 32 bits; 0 otherwise.
pub fn request_number(id: Option<&Value>) -> u32 {
    id.and_then(Value::as_i64).map(|n| n as u32).unwrap_or(0)
}

/// JSON text with sorted keys and whole floats written as integers.
pub fn text(value: &Value) -> String {
    let mut out = String::new();
    write(value, &mut out);
    out
}

fn write(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::from(key.as_str()).to_string());
                out.push(':');
                if let Some(inner) = map.get(*key) {
                    write(inner, out);
                }
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        Value::Number(n) if n.is_f64() => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => {
                out.push_str(&(f as i64).to_string())
            }
            _ => out.push_str(&n.to_string()),
        },
        other => out.push_str(&other.to_string()),
    }
}

/// Binary frames: 4 bytes of magic, a big-endian `u32`, then the JPEG.
pub mod frame {
    fn build(magic: &[u8; 4], number: u32, jpeg: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(8 + jpeg.len());
        data.extend_from_slice(magic);
        data.extend_from_slice(&number.to_be_bytes());
        data.extend_from_slice(jpeg);
        data
    }

    /// `PHN1`: a frame of the Mac's device window, by sequence.
    pub fn phone(sequence: u32, jpeg: &[u8]) -> Vec<u8> {
        build(b"PHN1", sequence, jpeg)
    }

    /// `IMG1`: the answer to `chat.image`, by request id.
    pub fn image(request_id: u32, jpeg: &[u8]) -> Vec<u8> {
        build(b"IMG1", request_id, jpeg)
    }

    /// `WEB1`: the answer to `web.snapshot`, by request id.
    pub fn snapshot(request_id: u32, jpeg: &[u8]) -> Vec<u8> {
        build(b"WEB1", request_id, jpeg)
    }
}
