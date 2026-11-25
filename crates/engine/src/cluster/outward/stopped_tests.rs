use super::{cut_steps, CutStep};
use crate::transcript::conversation::Parser;
use crate::transcript::model::Conversation;

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

fn launch(shell: &str, command: &str) -> String {
    call(
        &format!("t-{shell}"),
        "Bash",
        &format!(r#"{{"command":"{command}","description":"Run {shell}","run_in_background":true}}"#),
    ) + &result(
        &format!("t-{shell}"),
        &format!("Command running in background with ID: {shell}. Output is being written to: /tmp/t/{shell}.output"),
        false,
    )
}

fn said(n: usize) -> String {
    format!(r#"{{"type":"user","message":{{"role":"user","content":"hello {n}"}}}}"#) + "\n"
}

fn stop(name: &str, input: &str, is_error: bool) -> String {
    call("t-stop", name, input) + &result("t-stop", "reply", is_error)
}

fn parse(text: &str) -> Conversation {
    let mut parser = Parser::new();
    for line in text.lines() {
        parser.consume(line.as_bytes());
    }
    parser.conversation()
}

fn step(summary: &str, outward: bool) -> CutStep {
    CutStep {
        tool: "Bash".into(),
        summary: summary.into(),
        outward,
    }
}

#[test]
fn a_shell_stopped_before_the_kept_items_is_not_listed() {
    let mut text = said(0)
        + &launch("bgpush1", "git push origin main")
        + &launch("bgsleep2", "sleep 600")
        + &stop("TaskStop", r#"{"task_id":"bgpush1"}"#, false);
    for n in 1..1200 {
        text.push_str(&said(n));
    }
    let c = parse(&text);
    assert_eq!(c.shells.len(), 2);
    assert_eq!(cut_steps(&c), vec![step("Run bgsleep2", false)]);
}

#[test]
fn a_shell_stopped_by_taskstop_or_killshell_is_not_running() {
    for (name, input) in [
        ("TaskStop", r#"{"task_id":"b1"}"#),
        ("KillShell", r#"{"shell_id":"b1"}"#),
    ] {
        let text = said(0)
            + &launch("b1", "git push")
            + &launch("b2", "git push")
            + &stop(name, input, false);
        assert_eq!(
            cut_steps(&parse(&text)),
            vec![step("Run b2", true)],
            "{name}"
        );
        let text = said(0) + &launch("b1", "git push") + &stop(name, input, true);
        assert_eq!(cut_steps(&parse(&text)).len(), 1, "{name} failed");
    }
}
