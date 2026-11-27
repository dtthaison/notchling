use super::agentapi::{
    message_was_sent, new_conversation_args, new_conversation_id, reported_error,
    send_message_args, AgentApi, ApiError, RealAgentApi,
};
use std::path::PathBuf;
use std::time::Duration;

const ID: &str = "457369a9-35a1-40f8-92b9-26b6398b78a1";

#[test]
fn the_new_conversation_answer_is_read_and_its_id_checked() {
    let ok =
        format!(r#"{{"response":{{"newConversation":{{"prompt":"x","conversationId":"{ID}"}}}}}}"#);
    assert_eq!(new_conversation_id(&ok).unwrap(), ID);
    for bad in [
        "",
        "not json",
        "{}",
        r#"{"response":{"newConversation":{"conversationId":"x"}}}"#,
        r#"{"response":{"newConversation":{"conversationId":"../../etc"}}}"#,
    ] {
        assert!(new_conversation_id(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_send_answer_must_say_send_message() {
    assert!(message_was_sent(r#"{"response":{"sendMessage":{}}}"#));
    assert!(!message_was_sent(r#"{"response":{}}"#));
    assert!(!message_was_sent("oops"));
}

#[test]
fn an_error_answer_is_read_for_its_words() {
    assert_eq!(
        reported_error(r#"{"response":{},"error":"usage: x"}"#),
        Some("agentapi: usage: x".to_string())
    );
    assert_eq!(reported_error(r#"{"response":{"sendMessage":{}}}"#), None);
    assert_eq!(reported_error("oops"), None);
}

#[test]
fn the_id_comes_before_the_text_so_the_text_is_never_read_as_a_flag() {
    let args = send_message_args(ID, "--title=x -h").unwrap();
    assert_eq!(args, vec!["send-message", ID, "--title=x -h"]);
}

#[test]
fn the_arguments_are_fixed_and_the_text_is_one_argument() {
    assert_eq!(
        send_message_args(ID, "a b; c").unwrap(),
        vec!["send-message", ID, "a b; c"]
    );
    assert!(send_message_args("nope", "x").is_err());
    let args = new_conversation_args("p", Some("pro"), Some("T")).unwrap();
    assert_eq!(
        args,
        vec!["new-conversation", "--model=pro", "--title=T", "--", "p"]
    );
    assert!(new_conversation_args("p", Some("x y"), None).is_err());
    let long = "t".repeat(500);
    let args = new_conversation_args("p", None, Some(&long)).unwrap();
    assert_eq!(args[1].len(), "--title=".len() + 120);
}

#[cfg(unix)]
mod real {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nl-agentapi-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("agentapi");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn it_runs_the_program_with_the_arguments_and_no_shell() {
        let path = script("echo", r#"printf '%s|' "$@""#);
        let api = RealAgentApi::new(path);
        let out = api
            .run(&[
                "send-message".into(),
                "a; echo hacked".into(),
                "$(x)".into(),
            ])
            .unwrap();
        assert_eq!(out, "send-message|a; echo hacked|$(x)|");
    }

    #[test]
    fn a_failure_reports_its_first_line_and_a_missing_program_is_missing() {
        let path = script("fail", "echo 'no such thing' >&2; exit 3");
        let api = RealAgentApi::new(path);
        assert_eq!(
            api.run(&[]),
            Err(ApiError::Failed("agentapi: no such thing".into()))
        );
        let api = RealAgentApi::new(PathBuf::from("/nonexistent/agentapi"));
        assert_eq!(api.run(&[]), Err(ApiError::Missing));
    }

    #[test]
    fn a_file_that_cannot_run_never_started_so_it_is_not_started_and_not_a_failure() {
        let path = script("noexec", "exit 0");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let api = RealAgentApi::new(path);
        assert!(matches!(api.run(&[]), Err(ApiError::NotStarted(_))));
    }

    #[test]
    fn a_program_that_never_ends_is_killed_at_the_timeout() {
        let path = script("slow", "sleep 30");
        let api = RealAgentApi::new(path).with_timeout(Duration::from_millis(150));
        assert_eq!(api.run(&[]), Err(ApiError::TimedOut));
    }

    #[test]
    fn find_takes_the_first_agentapi_on_the_path_or_the_fallback() {
        let path = script("find", "true");
        let dir = path.parent().unwrap().to_string_lossy().into_owned();
        let fallback = PathBuf::from("/fallback/agentapi");
        assert_eq!(RealAgentApi::find(Some(&dir), fallback.clone()), path);
        assert_eq!(
            RealAgentApi::find(Some("/nonexistent"), fallback.clone()),
            fallback
        );
        assert_eq!(RealAgentApi::find(None, fallback.clone()), fallback);
    }
}
