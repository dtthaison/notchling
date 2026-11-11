//! Claude Code's own list of the models it offers beyond its aliases, `additionalModelOptionsCache`
//! in its main config file (`~/.claude.json`): the fallback for Claude's live list when
//! Anthropic's can't be read (no token, or the fetch failed). Read only: the file is Claude
//! Code's, and nothing here writes it.
//!
//! An entry looks like `{"value": "claude-fable-5-1[1m]", "label": "Fable", "description":
//! "Fable 5.1 · Most capable…"}`.

use std::path::Path;

use serde_json::Value;

use super::{model_key, startable_shape, LiveModel};

/// The key in Claude Code's config file.
pub const KEY: &str = "additionalModelOptionsCache";

/// The models in `config_file`, or none when it is missing, unreadable or has no list.
pub fn read(config_file: &Path) -> Vec<LiveModel> {
    std::fs::read(config_file)
        .map(|body| parse(&body))
        .unwrap_or_default()
}

/// Each entry whose `value` is a Claude model id in the shape one may start with, by its bare id
/// (`claude-fable-5-1[1m]` is `claude-fable-5-1`), once. Its name is the description's leading
/// model name ("Fable 5.1" from "Fable 5.1 · Most capable…") when it has one, else the label.
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
