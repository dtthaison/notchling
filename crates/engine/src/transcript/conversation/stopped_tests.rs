use super::Parser;
use crate::transcript::model::{Conversation, ItemKind};

fn call(id: &str, name: &str, input: &str) -> String {
    format!(
        r#"{{"type":"assistant","message":{{"id":"m-{id}","role":"assistant","content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{input}}}]}}}}"#
    ) + "\n"
}

fn result(id: &str, content: &str, is_error: bool) -> String {
    format!(
        r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"{content}","is_error":{is_error}}}]}}}}"#
    ) + "\n"
}

fn launch(shell: &str) -> String {
    call(
        "t-bg",
        "Bash",
        r#"{"command":"sleep 600","run_in_background":true}"#,
    ) + &result(
        "t-bg",
        &format!("Command running in background with ID: {shell}. Output is being written to: /tmp/t/{shell}.output"),
        false,
    )
}

fn said(n: usize) -> String {
    format!(r#"{{"type":"user","message":{{"role":"user","content":"hello {n}"}}}}"#) + "\n"
}

fn parse(text: &str) -> Conversation {
    let mut parser = Parser::new();
    for line in text.lines() {
        parser.consume(line.as_bytes());
    }
    parser.conversation()
}

fn stopped(name: &str, input: &str, shell: &str) -> Conversation {
    parse(
        &(launch(shell)
            + &call("t-stop", name, input)
            + &result("t-stop", "Successfully stopped task", false)),
    )
}

fn status<'a>(c: &'a Conversation, id: &str) -> Option<&'a str> {
    c.finished_agents.get(id).map(String::as_str)
}

#[test]
fn a_task_stop_result_finishes_the_shell() {
    let c = stopped("TaskStop", r#"{"task_id":"bg1"}"#, "bg1");
    assert_eq!(c.shells.len(), 1);
    assert_eq!(status(&c, "bg1"), Some("stopped"));
    assert_eq!(c.finished_agents.len(), 1);
}

#[test]
fn a_kill_shell_result_finishes_the_shell() {
    for key in ["shell_id", "bash_id"] {
        let c = stopped("KillShell", &format!(r#"{{"{key}":"bg2"}}"#), "bg2");
        assert_eq!(status(&c, "bg2"), Some("stopped"), "{key}");
    }
}

#[test]
fn a_stop_that_failed_records_nothing() {
    for name in ["TaskStop", "KillShell"] {
        let c = parse(
            &(launch("bg3")
                + &call("t-stop", name, r#"{"task_id":"bg3","shell_id":"bg3"}"#)
                + &result(
                    "t-stop",
                    "Task bg3 is not running (status: completed)",
                    true,
                )),
        );
        assert_eq!(c.shells.len(), 1);
        assert!(c.finished_agents.is_empty(), "{name}");
    }
}

#[test]
fn a_stop_without_an_id_or_by_another_tool_records_nothing() {
    let c = parse(
        &(launch("bg4")
            + &call("t-stop", "TaskStop", "{}")
            + &result("t-stop", "ok", false)
            + &call("t-read", "Read", r#"{"task_id":"bg4"}"#)
            + &result("t-read", "text", false)),
    );
    assert!(c.finished_agents.is_empty());
}

#[test]
fn a_stop_is_recorded_for_a_shell_launched_before_the_kept_items() {
    let mut text = launch("bg5");
    for n in 0..1200 {
        text.push_str(&said(n));
    }
    text.push_str(&call("t-stop", "TaskStop", r#"{"task_id":"bg5"}"#));
    text.push_str(&result("t-stop", "Successfully stopped task", false));
    let c = parse(&text);
    let launched = c.items.iter().any(|item| match &item.kind {
        ItemKind::Tools(calls) => calls.iter().any(|call| call.id == "t-bg"),
        _ => false,
    });
    assert!(!launched, "the launch is outside the kept items");
    assert_eq!(c.shells.len(), 1);
    assert_eq!(status(&c, "bg5"), Some("stopped"));
}

#[test]
fn a_stop_whose_call_was_trimmed_before_its_result_is_still_recorded() {
    let mut text = launch("bg6") + &call("t-stop", "TaskStop", r#"{"task_id":"bg6"}"#);
    for n in 0..1200 {
        text.push_str(&said(n));
    }
    text.push_str(&result("t-stop", "Successfully stopped task", false));
    let c = parse(&text);
    assert_eq!(status(&c, "bg6"), Some("stopped"));
}

#[test]
fn a_notification_and_a_stop_land_in_the_same_map() {
    let note = r#"{"type":"queue-operation","operation":"enqueue","content":"<task-notification>\n<task-id>bg7</task-id>\n<status>completed</status>\n</task-notification>"}"#;
    let c = parse(
        &(launch("bg8")
            + note
            + "\n"
            + &call("t-stop", "TaskStop", r#"{"task_id":"bg8"}"#)
            + &result("t-stop", "Successfully stopped task", false)),
    );
    assert_eq!(status(&c, "bg7"), Some("completed"));
    assert_eq!(status(&c, "bg8"), Some("stopped"));
}
