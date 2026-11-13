use serde_json::{json, Map, Value};

pub const DENIED: &str = "Denied from the notch.";

pub fn is_prompt_event(event: &Value) -> bool {
    let name = event.get("hook_event_name").and_then(Value::as_str);
    let tool = event.get("tool_name").and_then(Value::as_str);
    name == Some("PermissionRequest")
        || (name == Some("PreToolUse") && tool == Some("AskUserQuestion"))
}

pub fn always_rules(event: &Value) -> Vec<Value> {
    event
        .get("permission_suggestions")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter(|s| s.get("type").and_then(Value::as_str) != Some("setMode"))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub fn output(response: &Value, event: &Value) -> Option<Value> {
    let input = event
        .get("tool_input")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    match response.get("action").and_then(Value::as_str) {
        Some("answer") => {
            let mut updated = input;
            let answers: Map<String, Value> = response
                .get("answers")
                .and_then(Value::as_object)
                .filter(|m| m.values().all(Value::is_string))
                .cloned()
                .unwrap_or_default();
            updated.insert("answers".into(), Value::Object(answers));
            if event.get("hook_event_name").and_then(Value::as_str) == Some("PermissionRequest") {
                return Some(json!({"hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": {"behavior": "allow", "updatedInput": updated}}}));
            }
            Some(json!({"hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "updatedInput": updated}}))
        }
        Some("allow") => {
            let mut decision = json!({"behavior": "allow"});
            if response.get("always").and_then(Value::as_bool) == Some(true) {
                decision["updatedPermissions"] = Value::Array(always_rules(event));
            }
            Some(json!({"hookSpecificOutput": {
                "hookEventName": "PermissionRequest", "decision": decision}}))
        }
        Some("deny") => {
            let message = response
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or(DENIED);
            Some(json!({"hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": {"behavior": "deny", "message": message}}}))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/hook")
            .join(name);
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn only_permissions_and_questions_are_asked_about() {
        assert!(is_prompt_event(&fixture("permission-request.json")));
        assert!(is_prompt_event(&fixture("ask-user-question.json")));
        assert!(is_prompt_event(&fixture("ask-as-permission.json")));
        assert!(!is_prompt_event(&fixture("other-tool.json")));
        assert!(!is_prompt_event(&json!({})));
        assert!(!is_prompt_event(
            &json!({"hook_event_name": "PreToolUse", "tool_name": "Artifact"})
        ));
    }

    #[test]
    fn allow_always_and_deny_match_the_swift_output() {
        let event = fixture("permission-request.json");
        assert_eq!(
            output(&json!({"action": "allow"}), &event),
            Some(fixture("out-allow.json"))
        );
        assert_eq!(
            output(&json!({"action": "allow", "always": true}), &event),
            Some(fixture("out-always.json")),
            "always keeps rules and directories, never setMode"
        );
        assert_eq!(
            output(&json!({"action": "deny"}), &event),
            Some(fixture("out-deny.json"))
        );
        let told = output(&json!({"action": "deny", "message": "Not now."}), &event).unwrap();
        assert_eq!(
            told["hookSpecificOutput"]["decision"]["message"],
            json!("Not now.")
        );
    }

    #[test]
    fn answers_go_back_with_the_questions() {
        let answers =
            json!({"action": "answer", "answers": {"Which pets?": "Cat, Fish", "Where?": "Away"}});
        assert_eq!(
            output(&answers, &fixture("ask-user-question.json")),
            Some(fixture("out-answer-pretooluse.json"))
        );
        let answers = json!({"action": "answer", "answers": {"Where?": "Home"}});
        assert_eq!(
            output(&answers, &fixture("ask-as-permission.json")),
            Some(fixture("out-answer-permission.json"))
        );
    }

    #[test]
    fn the_swift_cases() {
        let pre = json!({"hook_event_name": "PreToolUse", "tool_input": {"questions": ["q"]}});
        let out = output(&json!({"action": "answer", "answers": {"q": "Yes"}}), &pre).unwrap();
        let specific = &out["hookSpecificOutput"];
        assert_eq!(specific["hookEventName"], "PreToolUse");
        assert_eq!(specific["permissionDecision"], "allow");
        assert_eq!(specific["updatedInput"]["answers"], json!({"q": "Yes"}));
        assert_eq!(specific["updatedInput"]["questions"], json!(["q"]));

        let perm = json!({"hook_event_name": "PermissionRequest", "tool_input": {}});
        let out = output(&json!({"action": "answer", "answers": {}}), &perm).unwrap();
        assert_eq!(out["hookSpecificOutput"]["decision"]["behavior"], "allow");

        let out = output(&json!({"action": "deny"}), &json!({})).unwrap();
        assert_eq!(
            out["hookSpecificOutput"]["decision"]["message"],
            "Denied from the notch."
        );
        let out = output(&json!({"action": "allow"}), &json!({})).unwrap();
        assert!(out["hookSpecificOutput"]["decision"]
            .get("updatedPermissions")
            .is_none());
        assert_eq!(output(&json!({"action": "pass"}), &json!({})), None);
        assert_eq!(output(&json!({}), &json!({})), None);
        assert_eq!(
            output(&json!({"action": "something new"}), &json!({})),
            None
        );
    }
}
