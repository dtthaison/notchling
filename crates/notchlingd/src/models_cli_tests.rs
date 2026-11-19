use super::*;
use notchling_local_api::ApiError;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn no_word_or_check_reads_and_update_writes() {
    let read = parse(&words(&[])).unwrap();
    assert!(!read.update && !read.json && read.skills);
    assert_eq!(request(&read).0, ops::MODELS_CHECK);
    assert!(parse(&words(&["check", "--json"])).unwrap().json);

    let write = parse(&words(&["update", "--no-skills", "--json"])).unwrap();
    assert!(write.update && write.json && !write.skills);
    let (op, args) = request(&write);
    assert_eq!(op, ops::MODELS_UPDATE);
    assert_eq!(args, json!({"skills": false}));
}

#[test]
fn a_wrong_command_gets_the_usage() {
    for bad in [
        vec!["refresh"],
        vec!["update", "now"],
        vec!["--nope"],
        vec!["--socket"],
    ] {
        let e = parse(&words(&bad)).unwrap_err();
        assert!(e.contains("Usage: notchling models"), "{bad:?}: {e}");
    }
    assert_eq!(
        parse(&words(&["--socket", "/tmp/s"])).unwrap().socket,
        Some(PathBuf::from("/tmp/s"))
    );
}

#[test]
fn the_text_is_the_report_text_and_json_is_the_whole_reply() {
    let reply = json!({"text": "Codex (x): 2 models", "applied": true});
    assert_eq!(shown(&reply, false), "Codex (x): 2 models");
    assert!(shown(&reply, true).contains("\"applied\": true"));
    assert!(shown(&json!({"applied": true}), false).contains("applied"));
}

#[test]
fn exit_codes_and_messages_follow_the_other_commands() {
    let api = |code, text: &str| ClientError::Api(ApiError::new(code, text));
    assert_eq!(code_of(&api(ErrorCode::Failed, "x")), REFUSED);
    assert_eq!(code_of(&api(ErrorCode::BadRequest, "x")), REFUSED);
    assert_eq!(code_of(&ClientError::Timeout), TROUBLE);
    assert_eq!(code_of(&ClientError::Unsupported), TROUBLE);
    assert!(message_of(ClientError::Unsupported).contains("isn't running"));
    assert!(message_of(api(ErrorCode::UnknownOp, "x")).contains("no models command yet"));
    assert_eq!(message_of(api(ErrorCode::Failed, "boom")), "boom");
}
