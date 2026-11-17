use super::util::str_of;
use serde_json::Value;

fn bare_prompt<'a>(line: &Value, text: &'a str) -> Option<&'a str> {
    let flagged = |key: &str| line.get(key).and_then(Value::as_bool) == Some(true);
    if flagged("isMeta") || flagged("isSidechain") || flagged("isCompactSummary") {
        return None;
    }
    let carries_result = line
        .pointer("/message/content")
        .and_then(Value::as_array)
        .is_some_and(|blocks| {
            blocks
                .iter()
                .any(|b| str_of(b, "type") == Some("tool_result"))
        });
    if carries_result {
        return None;
    }
    let text = text.trim();
    (!text.is_empty() && !text.starts_with("[Request interrupted")).then_some(text)
}

fn untagged(text: &str) -> bool {
    !text.starts_with('<') || text.starts_with("<bash-input>")
}

fn from_human(line: &Value) -> bool {
    line.pointer("/origin/kind")
        .or_else(|| line.pointer("/attachment/origin/kind"))
        .and_then(Value::as_str)
        == Some("human")
}

pub fn typed_prompt(line: &Value, text: &str) -> bool {
    bare_prompt(line, text).is_some_and(untagged)
}

pub fn human_prompt(line: &Value, text: &str) -> bool {
    bare_prompt(line, text).is_some_and(|text| {
        untagged(text) || (from_human(line) && !text.starts_with("<command-name>"))
    })
}
