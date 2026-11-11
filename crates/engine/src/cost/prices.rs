//! What Claude's token use would have cost at API prices (`CostTracker.cost`, `prices`).

use serde_json::Value;

/// Dollars per million tokens: input, output, cache read. Cache writes are 1.25x input for five
/// minutes and 2x for an hour. Checked in order, so longer ids come first.
const PRICES: &[(&str, f64, f64, f64)] = &[
    ("fable-5-2", 8.0, 40.0, 0.20),
    ("mythos-5-2", 8.0, 40.0, 0.20),
    ("fable-5-1", 10.0, 50.0, 0.25),
    ("mythos-5-1", 10.0, 50.0, 0.25),
    ("fable", 10.0, 50.0, 1.0),
    ("mythos", 10.0, 50.0, 1.0),
    ("opus-5-5", 4.0, 20.0, 0.20),
    ("opus-5", 5.0, 25.0, 0.50),
    ("opus-4-8", 5.0, 25.0, 0.50),
    ("opus-4-7", 5.0, 25.0, 0.50),
    ("opus-4-6", 5.0, 25.0, 0.50),
    ("opus-4-5", 5.0, 25.0, 0.50),
    ("opus-4", 15.0, 75.0, 1.50),
    ("sonnet-5", 2.0, 10.0, 0.20),
    ("sonnet", 3.0, 15.0, 0.30),
    ("haiku-5-5", 0.10, 0.50, 0.01),
    ("haiku-4-5", 1.0, 5.0, 0.10),
    ("haiku", 0.80, 4.0, 0.08),
];

fn n(from: &Value, key: &str) -> f64 {
    from.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

/// Input, output, cache-read and cache-write token counts from one message's usage.
pub fn tokens(usage: &Value) -> [f64; 4] {
    let (write5m, write1h) = cache_writes(usage);
    [
        n(usage, "input_tokens"),
        n(usage, "output_tokens"),
        n(usage, "cache_read_input_tokens"),
        write5m + write1h,
    ]
}

/// Five-minute and one-hour cache writes: the split when the usage has one, else all five-minute.
fn cache_writes(usage: &Value) -> (f64, f64) {
    match usage.get("cache_creation").filter(|v| v.is_object()) {
        Some(split) => (
            n(split, "ephemeral_5m_input_tokens"),
            n(split, "ephemeral_1h_input_tokens"),
        ),
        None => (n(usage, "cache_creation_input_tokens"), 0.0),
    }
}

/// Dollars for one message; 0 for a model with no price here.
pub fn cost(model: &str, usage: &Value) -> f64 {
    let id = model_key(model).replace("claude-", "");
    let Some(&(prefix, mut input, mut output, mut read)) =
        PRICES.iter().find(|p| id.starts_with(p.0))
    else {
        return 0.0;
    };
    let in_tokens = n(usage, "input_tokens");
    let out_tokens = n(usage, "output_tokens");
    let read_tokens = n(usage, "cache_read_input_tokens");
    let (write5m, write1h) = cache_writes(usage);
    // Haiku 5.5 costs five times as much once the prompt is over 100K tokens.
    if prefix == "haiku-5-5" && in_tokens + read_tokens + write5m + write1h > 100_000.0 {
        (input, output, read) = (0.50, 2.50, 0.05);
    }
    let mut dollars = in_tokens * input
        + out_tokens * output
        + read_tokens * read
        + write5m * input * 1.25
        + write1h * input * 2.0;
    if usage.get("speed").and_then(Value::as_str) == Some("fast") {
        dollars *= 2.0;
    }
    dollars / 1_000_000.0
}

/// "claude-opus-5-5[1m]" and "claude-haiku-4-5-20251001" are "claude-opus-5-5" and
/// "claude-haiku-4-5".
pub fn model_key(model: &str) -> String {
    let mut id = model;
    if let Some(i) = id.find('[') {
        id = &id[..i];
    }
    if let Some(i) = id.len().checked_sub(9) {
        let (head, tail) = id.split_at_checked(i).unwrap_or((id, ""));
        let dated = tail.len() == 9
            && tail.starts_with('-')
            && tail[1..].bytes().all(|b| b.is_ascii_digit());
        if dated {
            return head.to_string();
        }
    }
    id.to_string()
}

/// "claude-opus-5-5" is "Opus 5.5". Other tools' models ("gpt-6.1-sol", "Gemini 4 Argon") are
/// already named.
pub fn display_name(key: &str) -> String {
    let Some(rest) = key.strip_prefix("claude-") else {
        return key.to_string();
    };
    let mut parts = rest.split('-');
    let Some(family) = parts.next().filter(|f| !f.is_empty()) else {
        return key.to_string();
    };
    let version: Vec<&str> = parts.collect();
    let mut chars = family.chars();
    let head: String = chars
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    let name = format!("{head}{}", chars.as_str());
    if version.is_empty() {
        name
    } else {
        format!("{name} {}", version.join("."))
    }
}
