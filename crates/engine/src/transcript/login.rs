use super::restart::{flag, WINDOW_BYTES};
use super::tail::read_lines;
use super::util::{parse_object, str_of};
use serde_json::Value;

const SIGNED_IN_LINE: &str = "<local-command-stdout>Login successful</local-command-stdout>";

const AUTH_WORDS: [&str; 7] = [
    "/login",
    "authentication_error",
    "oauth token has expired",
    "oauth access token has been revoked",
    "invalid api key",
    "not logged in",
    "invalid bearer token",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Needed,
    Cleared,
}

fn says_auth(text: &str) -> bool {
    let lower = text.to_lowercase();
    AUTH_WORDS.iter().any(|word| lower.contains(word))
}

fn signed_out(status: Option<u64>, said: impl FnOnce() -> bool) -> bool {
    match status {
        Some(code) => code == 401,
        None => said(),
    }
}

fn text_of(obj: &Value) -> String {
    match obj.pointer("/message/content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| str_of(b, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn assistant(obj: &Value) -> Option<Step> {
    if !flag(obj, "isApiErrorMessage") {
        let synthetic =
            obj.pointer("/message/model").and_then(Value::as_str) == Some("<synthetic>");
        return (!synthetic).then_some(Step::Cleared);
    }
    let status = obj.get("apiErrorStatus").and_then(Value::as_u64);
    Some(if signed_out(status, || says_auth(&text_of(obj))) {
        Step::Needed
    } else {
        Step::Cleared
    })
}

fn retry(obj: &Value) -> Option<Step> {
    if str_of(obj, "subtype") != Some("api_error") {
        return None;
    }
    let error = obj.get("error")?;
    let status = error.get("status").and_then(Value::as_u64);
    let said = || {
        ["message", "formatted"]
            .iter()
            .filter_map(|key| str_of(error, key))
            .any(says_auth)
    };
    signed_out(status, said).then_some(Step::Needed)
}

fn user(obj: &Value) -> Option<Step> {
    (text_of(obj).trim() == SIGNED_IN_LINE).then_some(Step::Cleared)
}

pub fn step(obj: &Value) -> Option<Step> {
    if flag(obj, "isSidechain") {
        return None;
    }
    match str_of(obj, "type") {
        Some("assistant") => assistant(obj),
        Some("system") => retry(obj),
        Some("user") => user(obj),
        _ => None,
    }
}

pub fn login_needed_in<'a>(lines: impl DoubleEndedIterator<Item = &'a str>) -> bool {
    for line in lines.rev() {
        let Some(map) = parse_object(line.as_bytes()) else {
            continue;
        };
        if let Some(step) = step(&Value::Object(map)) {
            return step == Step::Needed;
        }
    }
    false
}

pub fn login_needed_in_file(path: &str) -> bool {
    read_lines(path, WINDOW_BYTES)
        .map(|lines| login_needed_in(lines.iter().map(String::as_str)))
        .unwrap_or(false)
}
