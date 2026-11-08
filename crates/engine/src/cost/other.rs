//! Token use from the other tools' session files (`CostSources.swift`). Each parser is pure (one
//! decoded line in, an event out) so it can be tested without a tracker.

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

/// Dollars per million tokens for models outside Claude, at OpenAI's API list prices: input,
/// cached input, output. Source: openai.com/api/pricing, as of 2026-11-06. A model not listed (the
/// gpt-6.x family, Gemini) has no entry because no list price is published for it that Notchling
/// knows: its tokens are still counted and its cost is 0. Add a line here when a price is known.
/// Ids match whole, minus a `-YYYY-MM-DD` suffix, so "gpt-5.6-sol" can't pick up "gpt-5"'s price.
pub struct OtherPrices;

impl OtherPrices {
    pub const TABLE: &'static [(&'static str, f64, f64, f64)] = &[
        ("gpt-5", 1.25, 0.125, 10.0),
        ("gpt-5-codex", 1.25, 0.125, 10.0),
        ("gpt-5.1", 1.25, 0.125, 10.0),
        ("gpt-5.1-codex", 1.25, 0.125, 10.0),
        ("gpt-5.1-codex-max", 1.25, 0.125, 10.0),
        ("gpt-5-mini", 0.25, 0.025, 2.0),
        ("gpt-5.1-codex-mini", 0.25, 0.025, 2.0),
        ("gpt-5-nano", 0.05, 0.005, 0.40),
        ("gpt-4.1", 2.0, 0.50, 8.0),
        ("o3", 2.0, 0.50, 8.0),
        ("o4-mini", 1.10, 0.275, 4.40),
    ];

    /// Dollars for one request. `counts` is input (not cached), output, cache read.
    pub fn cost(model: &str, counts: &[f64]) -> f64 {
        let lower = model.to_lowercase();
        let id = strip_date_suffix(&lower);
        let Some(&(_, input, cached, output)) = Self::TABLE.iter().find(|p| p.0 == id) else {
            return 0.0;
        };
        if counts.len() < 3 {
            return 0.0;
        }
        (counts[0] * input + counts[2] * cached + counts[1] * output) / 1_000_000.0
    }
}

/// "gpt-5.1-2026-01-01" is "gpt-5.1".
fn strip_date_suffix(id: &str) -> &str {
    let bytes = id.as_bytes();
    if bytes.len() < 11 {
        return id;
    }
    let tail = &bytes[bytes.len() - 11..];
    let digits = |r: std::ops::Range<usize>| tail[r].iter().all(u8::is_ascii_digit);
    if tail[0] == b'-'
        && digits(1..5)
        && tail[5] == b'-'
        && digits(6..8)
        && tail[8] == b'-'
        && digits(9..11)
    {
        &id[..id.len() - 11]
    } else {
        id
    }
}
