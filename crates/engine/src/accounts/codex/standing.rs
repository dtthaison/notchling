use crate::providers::usage::{Credits, Reached, Standing};
use serde_json::Value;

const MAX_BALANCE_CHARS: usize = 16;

pub fn standing(data: &[u8]) -> Option<Standing> {
    let root: Value = serde_json::from_slice(data).ok()?;
    let standing = Standing {
        reached: reached(&root),
        credits: credits(&root),
    };
    (!standing.is_empty()).then_some(standing)
}

fn reached(root: &Value) -> Option<Reached> {
    let kind = root
        .pointer("/rate_limit_reached_type/type")
        .and_then(Value::as_str);
    if let Some(kind) = kind {
        return Some(if kind.contains("credits") {
            Reached::Credits
        } else {
            Reached::Limit
        });
    }
    let flag = |key: &str, want: bool| {
        root.pointer(&format!("/rate_limit/{key}"))
            .and_then(Value::as_bool)
            == Some(want)
    };
    (flag("allowed", false) || flag("limit_reached", true)).then_some(Reached::Limit)
}

fn credits(root: &Value) -> Option<Credits> {
    let credits = root.get("credits").filter(|c| c.is_object())?;
    let unlimited = credits.get("unlimited").and_then(Value::as_bool) == Some(true);
    let balance = credits.get("balance").and_then(balance);
    (unlimited || balance.is_some()).then_some(Credits { unlimited, balance })
}

fn balance(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(s) => s.trim().to_owned(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let plain = !text.is_empty()
        && text.chars().count() <= MAX_BALANCE_CHARS
        && text.chars().all(|c| c.is_ascii_digit() || c == '.')
        && text.chars().filter(|c| *c == '.').count() <= 1
        && text.chars().any(|c| c.is_ascii_digit());
    let positive = text.chars().any(|c| matches!(c, '1'..='9'));
    (plain && positive).then_some(text)
}
