use super::decide::Kind;
use serde_json::{json, Value};

pub fn state_json(holding: Option<Kind>) -> Value {
    json!({"holding": holding.map(Kind::as_str)})
}

pub fn holding_in(value: &Value) -> Option<Option<Kind>> {
    match value.get("holding")? {
        Value::Null => Some(None),
        Value::String(word) => Kind::parse(word).map(Some),
        _ => None,
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
