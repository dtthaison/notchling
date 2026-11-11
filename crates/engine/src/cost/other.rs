//! Token use from the other tools' session files (`CostSources.swift`). Each parser is pure (one
//! decoded line in, an event out) so it can be tested without a tracker.

use crate::catalog::{Catalog, Tool};
use crate::transcript::util::parse_date;
use serde_json::Value;

/// Codex's `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. Three kinds of line matter:
/// - `session_meta`: `creator_user_id` and `creator_account_id` name the login (the id Codex's
///   store uses).
/// - `turn_context`: the `model` and `cwd` the following requests run with.
/// - `event_msg` / `token_count`: `info.last_token_usage` is what the latest request used and
///   `info.total_token_usage` the running sum. Only `last_token_usage` is counted; the running sum
///   is used to drop an event Codex wrote twice (same total as the one before it).
///
/// `input_tokens` includes `cached_input_tokens`, and `output_tokens` includes the reasoning
/// tokens.
pub mod codex {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    pub struct Tokens {
        /// `total_token_usage.total_tokens`, -1 when the line has none.
        pub total: f64,
        pub input: f64,
        pub cached: f64,
        pub output: f64,
        pub date: f64,
        pub ordinal: Option<f64>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum Event {
        Session {
            account: Option<String>,
        },
        Turn {
            model: Option<String>,
            folder: Option<String>,
        },
        Tokens(Tokens),
    }

    fn n(from: &Value, key: &str) -> f64 {
        from.get(key).and_then(Value::as_f64).unwrap_or(0.0)
    }

    pub fn parse(object: &Value) -> Option<Event> {
        let payload = object.get("payload")?.as_object()?;
        let payload = Value::Object(payload.clone());
        match object.get("type").and_then(Value::as_str)? {
            "session_meta" => {
                let user = payload.get("creator_user_id").and_then(Value::as_str);
                let account = payload.get("creator_account_id").and_then(Value::as_str);
                Some(Event::Session {
                    account: user.zip(account).map(|(u, a)| format!("{u}:{a}")),
                })
            }
            "turn_context" => Some(Event::Turn {
                model: payload
                    .get("model")
                    .and_then(Value::as_str)
                    .map(String::from),
                folder: payload.get("cwd").and_then(Value::as_str).map(String::from),
            }),
            "event_msg" => {
                if payload.get("type").and_then(Value::as_str) != Some("token_count") {
                    return None;
                }
                let info = payload.get("info")?;
                let last = info.get("last_token_usage").filter(|v| v.is_object())?;
                let date = parse_date(object.get("timestamp")?.as_str()?)?;
                let total = info
                    .get("total_token_usage")
                    .filter(|v| v.is_object())
                    .map_or(-1.0, |t| n(t, "total_tokens"));
                Some(Event::Tokens(Tokens {
                    total,
                    input: n(last, "input_tokens"),
                    cached: n(last, "cached_input_tokens"),
                    output: n(last, "output_tokens"),
                    date,
                    ordinal: object.get("ordinal").and_then(Value::as_f64),
                }))
            }
            _ => None,
        }
    }

    /// Input (not cached), output, cache read, cache write. OpenAI has no cache-write charge.
    pub fn counts(t: &Tokens) -> [f64; 4] {
        [(t.input - t.cached).max(0.0), t.output, t.cached, 0.0]
    }
}

/// Antigravity CLI's `brain/<conversation>/.system_generated/logs/transcript.jsonl`. A
/// `PLANNER_RESPONSE` step carries `input_tokens` (not counting the cache), `cache_read_tokens` and
/// `output_tokens` for its request, without a model. The model is the last "Model Selection"
/// setting change in a `USER_INPUT` step ("changed setting `Model Selection` from None to Gemini 4
/// Argon (Medium)").
pub mod antigravity {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    pub struct Tokens {
        pub input: f64,
        pub cache_read: f64,
        pub output: f64,
        pub date: f64,
        pub step: f64,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum Event {
        Model(String),
        Tokens(Tokens),
    }

    pub fn parse(object: &Value) -> Option<Event> {
        match object.get("type").and_then(Value::as_str)? {
            "USER_INPUT" => model_in(object.get("content")?.as_str()?).map(Event::Model),
            "PLANNER_RESPONSE" => {
                let input = object.get("input_tokens")?.as_f64()?;
                let step = object.get("step_index")?.as_f64()?;
                let date = parse_date(object.get("created_at")?.as_str()?)?;
                let n = |key: &str| object.get(key).and_then(Value::as_f64).unwrap_or(0.0);
                Some(Event::Tokens(Tokens {
                    input,
                    cache_read: n("cache_read_tokens"),
                    output: n("output_tokens"),
                    date,
                    step,
                }))
            }
            _ => None,
        }
    }

    /// "Gemini 4 Argon" from "...changed setting `Model Selection` from None to Gemini 4 Argon
    /// (Medium). No need...".
    pub fn model_in(content: &str) -> Option<String> {
        let marker = "`Model Selection` from ";
        let rest = &content[content.find(marker)? + marker.len()..];
        // " to " after a non-empty "from" value, then the name up to an optional " (Level)" and
        // ". No need".
        let end = rest.find(". No need")?;
        let head = &rest[..end];
        let to = head
            .char_indices()
            .filter(|(i, _)| head[*i..].starts_with(" to "))
            .find(|(i, _)| *i > 0)?
            .0;
        let mut name = &head[to + 4..];
        if let Some(open) = name.rfind(" (") {
            let level = &name[open + 2..];
            if level.ends_with(')')
                && level[..level.len() - 1]
                    .chars()
                    .all(|c| c.is_ascii_alphabetic() || c == ' ')
            {
                name = &name[..open];
            }
        }
        (!name.is_empty()).then(|| name.to_string())
    }

    pub fn counts(t: &Tokens) -> [f64; 4] {
        [t.input, t.output, t.cache_read, 0.0]
    }
}

/// Dollars for a request by a model outside Claude, at the catalog's price (`Catalog::price`
/// matches an id whole, minus a `-YYYY-MM-DD` suffix, so "gpt-5.6-sol" can't pick up "gpt-5"'s).
/// A model with no price (the gpt-6.x family, Gemini) is `None`: its tokens are still counted and
/// it is listed as unpriced.
pub struct OtherPrices;

impl OtherPrices {
    /// Dollars for one request. `counts` is input (not cached), output, cache read.
    pub fn cost(catalog: &Catalog, tool: Tool, model: &str, counts: &[f64]) -> Option<f64> {
        let price = catalog.price(tool, model)?;
        if counts.len() < 3 {
            return None;
        }
        Some(
            (counts[0] * price.input + counts[2] * price.cache_read + counts[1] * price.output)
                / 1_000_000.0,
        )
    }
}
