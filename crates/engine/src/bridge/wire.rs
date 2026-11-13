use notchling_protocol::generated::ErrorCode;
use serde::Serialize;
use serde_json::{Map, Value};

pub const VERSION: i64 = 1;

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

pub fn code_name(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Unauthorized => "unauthorized",
        ErrorCode::NotFound => "not_found",
        ErrorCode::ViewOnly => "view_only",
        ErrorCode::Invalid => "invalid",
        ErrorCode::Busy => "busy",
        ErrorCode::NotReachable => "not_reachable",
        ErrorCode::Internal | ErrorCode::Unknown => "internal",
    }
}

pub fn ok(id: Option<&Value>, data: Value) -> String {
    let mut object = Map::new();
    object.insert("ok".into(), Value::Bool(true));
    object.insert("data".into(), data);
    if let Some(id) = id {
        object.insert("id".into(), id.clone());
    }
    text(&Value::Object(object))
}

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

pub fn event(name: &str, mut fields: Map<String, Value>) -> String {
    fields.insert("event".into(), Value::from(name));
    text(&Value::Object(fields))
}

pub fn object<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

pub fn request_number(id: Option<&Value>) -> u32 {
    id.and_then(Value::as_i64).map(|n| n as u32).unwrap_or(0)
}

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

pub mod frame {
    fn build(magic: &[u8; 4], number: u32, jpeg: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(8 + jpeg.len());
        data.extend_from_slice(magic);
        data.extend_from_slice(&number.to_be_bytes());
        data.extend_from_slice(jpeg);
        data
    }

    pub fn phone(sequence: u32, jpeg: &[u8]) -> Vec<u8> {
        build(b"PHN1", sequence, jpeg)
    }

    pub fn image(request_id: u32, jpeg: &[u8]) -> Vec<u8> {
        build(b"IMG1", request_id, jpeg)
    }

    pub fn snapshot(request_id: u32, jpeg: &[u8]) -> Vec<u8> {
        build(b"WEB1", request_id, jpeg)
    }
}
