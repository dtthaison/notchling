use super::util::str_of;
use serde_json::Value;

pub fn typed_prompt(line: &Value, text: &str) -> bool {
    let flagged = |key: &str| line.get(key).and_then(Value::as_bool) == Some(true);
    if flagged("isMeta") || flagged("isSidechain") || flagged("isCompactSummary") {
        return false;
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
        return false;
    }
    let text = text.trim();
    !text.is_empty()
        && !text.starts_with("[Request interrupted")
        && (!text.starts_with('<') || text.starts_with("<bash-input>"))
}
