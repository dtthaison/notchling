use serde_json::Value;

pub fn parse_written(text: &str) -> Option<Value> {
    serde_json::from_str(text).ok()
}
