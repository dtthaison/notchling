//! What Claude's token use would have cost at API prices (`CostTracker.cost`, `prices`).

use crate::catalog::{Catalog, Tool};
use serde_json::Value;

// The catalog owns it now; cost and the sources still ask here.
pub use crate::catalog::model_key;

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

/// Dollars for one message, or `None` for a model the catalog has no price for.
pub fn cost(catalog: &Catalog, model: &str, usage: &Value) -> Option<f64> {
    let price = catalog.price(Tool::Claude, model)?;
    let in_tokens = n(usage, "input_tokens");
    let out_tokens = n(usage, "output_tokens");
    let read_tokens = n(usage, "cache_read_input_tokens");
    let (write5m, write1h) = cache_writes(usage);
    let (mut input, mut output, mut read) = (price.input, price.output, price.cache_read);
    // A long prompt (Haiku 5.5's) costs the dearer tier.
    if let Some(over) = price.over {
        if in_tokens + read_tokens + write5m + write1h > over.tokens {
            (input, output, read) = (over.input, over.output, over.cache_read);
        }
    }
    let mut dollars = in_tokens * input
        + out_tokens * output
        + read_tokens * read
        + write5m * input * 1.25
        + write1h * input * 2.0;
    if usage.get("speed").and_then(Value::as_str) == Some("fast") {
        dollars *= 2.0;
    }
    Some(dollars / 1_000_000.0)
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
