use super::build;
use notchling_engine::cluster::wire::OriginTier;
use notchling_local_api::ErrorCode;
use serde_json::{json, Value};

const NODE: &str = "0123456789abcdef";

fn claude(extra: Value) -> Value {
    let mut args = json!({"node": NODE, "tool": "claude", "cwd": "/srv", "key": "k"});
    for (name, value) in extra.as_object().unwrap() {
        args[name] = value.clone();
    }
    args
}

#[test]
fn a_claude_session_takes_its_task_as_text_next_to_its_model() {
    let (_, spec) = build(&claude(json!({"text": "fix the build"})), OriginTier::App).unwrap();
    assert_eq!(spec.args, json!({"text": "fix the build"}));
    let both = claude(json!({"text": "fix the build", "model": "opus"}));
    let (_, spec) = build(&both, OriginTier::App).unwrap();
    assert_eq!(spec.args["text"], "fix the build");
    assert_eq!(spec.args["flags"], json!(["--model", "opus"]));
}

#[test]
fn a_claude_session_without_text_still_starts_with_nothing_else() {
    let (_, spec) = build(&claude(json!({})), OriginTier::App).unwrap();
    assert_eq!(spec.args, json!({}));
}

#[test]
fn a_blank_or_oversized_claude_task_and_a_command_are_refused() {
    let huge = "x".repeat(64 * 1024 + 1);
    for extra in [
        json!({"text": ""}),
        json!({"text": "  \n "}),
        json!({"text": huge}),
        json!({"text": "go", "command": "id"}),
    ] {
        let got = build(&claude(extra.clone()), OriginTier::App);
        assert_eq!(got.unwrap_err().code, ErrorCode::BadRequest, "{extra}");
    }
}
