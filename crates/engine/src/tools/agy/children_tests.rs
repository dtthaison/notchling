use super::*;
use crate::sessions::testutil::TempDir;

fn step(index: u32, kind: &str, extra: &str) -> String {
    format!(
        r#"{{"step_index":{index},"source":"MODEL","type":"{kind}","status":"DONE","created_at":"2026-11-14T09:00:00Z"{extra}}}"#
    )
}

fn planner(index: u32, calls: &str) -> String {
    step(
        index,
        "PLANNER_RESPONSE",
        &format!(r#","tool_calls":[{calls}]"#),
    )
}

fn write(dir: &TempDir, name: &str, lines: &[String]) -> std::path::PathBuf {
    dir.write(name, &(lines.join("\n") + "\n"))
}

#[test]
fn a_child_that_called_send_message_has_sent() {
    let dir = TempDir::new("kids-sent");
    let sent = write(
        &dir,
        "sent.jsonl",
        &[
            planner(0, r#"{"name":"view_file"}"#),
            planner(
                1,
                r#"{"name":"send_message","args":{"Message":"\"done\""}}"#,
            ),
        ],
    );
    let quiet = write(
        &dir,
        "quiet.jsonl",
        &[planner(0, r#"{"name":"view_file"}"#)],
    );
    assert_eq!(
        signals(&sent),
        Signals {
            sent: true,
            failed: false
        }
    );
    assert_eq!(signals(&quiet), Signals::default());
    assert_eq!(signals(&dir.path("missing.jsonl")), Signals::default());
}

#[test]
fn a_log_ending_in_an_error_step_has_failed() {
    let dir = TempDir::new("kids-failed");
    let failed = write(
        &dir,
        "failed.jsonl",
        &[
            planner(0, r#"{"name":"view_file"}"#),
            step(1, "ERROR_MESSAGE", r#","content":"quota""#),
        ],
    );
    let recovered = write(
        &dir,
        "recovered.jsonl",
        &[
            step(0, "ERROR_MESSAGE", r#","content":"quota""#),
            planner(1, r#"{"name":"view_file"}"#),
        ],
    );
    assert!(signals(&failed).failed);
    assert!(!signals(&recovered).failed);
}

#[test]
fn the_parent_hears_from_the_senders_named_in_its_messages() {
    let dir = TempDir::new("kids-heard");
    let parent = write(
        &dir,
        "parent.jsonl",
        &[
            step(
                3,
                "SYSTEM_MESSAGE",
                r#","content":"[Message] from a subagent. sender=11111111-aaaa-4bbb-8ccc-222222222222 text=hello""#,
            ),
            step(4, "SYSTEM_MESSAGE", r#","content":"server restart""#),
            step(
                5,
                "SYSTEM_MESSAGE",
                r#","content":"[Message] sender=other-child, done""#,
            ),
        ],
    );
    let heard = heard_from(&parent);
    assert!(heard.contains("11111111-aaaa-4bbb-8ccc-222222222222"));
    assert!(heard.contains("other-child"));
    assert_eq!(heard.len(), 2);
    assert!(heard_from(&dir.path("missing.jsonl")).is_empty());
}

#[test]
fn roles_come_from_the_parents_invoke_subagent_calls_in_order() {
    let dir = TempDir::new("kids-roles");
    let parent = write(
        &dir,
        "parent.jsonl",
        &[
            planner(
                0,
                r#"{"name":"invoke_subagent","args":{"role":"\"Reviewer\\nreads the diff\"","prompt":"\"x\""}}"#,
            ),
            planner(1, r#"{"name":"view_file"}"#),
            planner(
                2,
                r#"{"name":"invoke_subagent","args":{"role":"\"Image maker\""}},{"name":"invoke_subagent","args":{"prompt":"\"no role\""}}"#,
            ),
        ],
    );
    assert_eq!(roles(&parent), ["Reviewer", "Image maker", ""]);
    assert!(roles(&dir.path("missing.jsonl")).is_empty());
}

#[test]
fn the_first_step_time_is_when_the_child_started() {
    let dir = TempDir::new("kids-started");
    let log = write(
        &dir,
        "log.jsonl",
        &[step(0, "USER_INPUT", r#","content":"hi""#)],
    );
    assert_eq!(started(&log), Some(1_794_646_800.0));
    assert_eq!(started(&dir.path("missing.jsonl")), None);
}
