use super::*;

#[test]
fn a_new_session_is_named_by_thread_started() {
    assert_eq!(
        parse_line(r#"{"type":"thread.started","thread_id":"019a5c3e-12ab"}"#),
        Event::ThreadStarted {
            thread_id: "019a5c3e-12ab".into()
        }
    );
    // No id, or an empty one, names nothing.
    assert_eq!(parse_line(r#"{"type":"thread.started"}"#), Event::Other);
    assert_eq!(
        parse_line(r#"{"type":"thread.started","thread_id":""}"#),
        Event::Other
    );
    assert_eq!(
        parse_line(r#"{"type":"thread.started","thread_id":7}"#),
        Event::Other
    );
}

#[test]
fn errors_carry_their_message_from_either_shape() {
    assert_eq!(
        parse_line(r#"{"type":"error","message":"stream disconnected"}"#),
        Event::Failed {
            message: Some("stream disconnected".into())
        }
    );
    assert_eq!(
        parse_line(r#"{"type":"turn.failed","error":{"message":"rate limited"}}"#),
        Event::Failed {
            message: Some("rate limited".into())
        }
    );
    assert_eq!(
        parse_line(r#"{"type":"turn.failed"}"#),
        Event::Failed { message: None }
    );
}

#[test]
fn progress_events_are_named() {
    assert_eq!(parse_line(r#"{"type":"turn.started"}"#), Event::TurnStarted);
    assert_eq!(
        parse_line(r#"{"type":"turn.completed","usage":{"input_tokens":10}}"#),
        Event::TurnCompleted
    );
    assert_eq!(
        parse_line(
            r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"hi"}}"#
        ),
        Event::Item {
            phase: "completed".into(),
            kind: "agent_message".into()
        }
    );
    assert_eq!(
        parse_line(r#"{"type":"item.started","item":{"type":"command_execution"}}"#),
        Event::Item {
            phase: "started".into(),
            kind: "command_execution".into()
        }
    );
}

#[test]
fn lines_that_are_not_events_are_other() {
    for line in [
        "",
        "Reading prompt from stdin...",
        "[1, 2]",
        "{\"no\":\"type\"}",
        "{\"type\":5}",
        "{\"type\":\"future.event\"}",
        "{\"type\":\"thread.started\"",
    ] {
        assert_eq!(parse_line(line), Event::Other, "{line:?}");
    }
}

#[test]
fn a_line_over_the_cap_is_not_parsed() {
    let long = format!(
        r#"{{"type":"thread.started","thread_id":"{}"}}"#,
        "a".repeat(MAX_LINE_BYTES)
    );
    assert_eq!(parse_line(&long), Event::Other);
}
