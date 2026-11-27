use std::path::Path;

use serde_json::Value;

use super::{model_key, startable_shape, LiveModel};

pub const KEY: &str = "additionalModelOptionsCache";

pub fn read(config_file: &Path) -> Vec<LiveModel> {
    std::fs::read(config_file)
        .map(|body| parse(&body))
        .unwrap_or_default()
}

pub fn parse(body: &[u8]) -> Vec<LiveModel> {
    let Ok(Value::Object(config)) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let Some(Value::Array(options)) = config.get(KEY) else {
        return Vec::new();
    };
    let mut out: Vec<LiveModel> = Vec::new();
    for option in options {
        let Some(value) = option.get("value").and_then(Value::as_str) else {
            continue;
        };
        if !value.starts_with("claude-") || !startable_shape(value) {
            continue;
        }
        let id = model_key(value);
        if out.iter().any(|m| m.id == id) {
            continue;
        }
        let text = |key: &str| {
            option
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
        };
        let named = text("description")
            .and_then(|d| d.split_once(" · "))
            .map(|(head, _)| head.trim())
            .filter(|head| !head.is_empty() && head.chars().any(|c| c.is_ascii_digit()));
        out.push(LiveModel {
            id,
            name: named.or_else(|| text("label")).map(str::to_string),
            ..LiveModel::default()
        });
    }
    out
}
