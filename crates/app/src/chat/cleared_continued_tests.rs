use super::*;
use crate::testutil::Scratch;
use std::time::Duration;

fn tick() {
    std::thread::sleep(Duration::from_millis(50));
}

fn named(agent: &str) -> String {
    format!(r#"{{"type":"agent-name","agentName":"{agent}"}}"#) + "\n"
}

fn cleared(agent: &str) -> String {
    format!(r#"{{"type":"system","agentName":"{agent}","content":"SessionStart:clear"}}"#) + "\n"
}

fn reply(model: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"a1","timestamp":"2026-11-12T10:00:00Z","message":{{"id":"m1","role":"assistant","model":"{model}","content":[{{"type":"text","text":"hi"}}]}}}}"#
    ) + "\n"
}

fn continued(id: &str, to: &str) -> String {
    format!(r#"{{"type":"continued-in","sessionId":"{id}","continuedInSessionId":"{to}"}}"#) + "\n"
}

#[test]
fn a_clear_after_a_move_keeps_the_model_the_session_ran_with_before_the_move() {
    let s = Scratch::new("cleared-after-move");
    s.write(
        "p/w.jsonl",
        &(named("worker") + &reply("claude-opus-5-5") + &continued("w", "x")),
    );
    tick();
    s.write("p/x.jsonl", &named("worker"));
    tick();
    let y = s.write("p/y.jsonl", &cleared("worker"));
    let path = y.to_string_lossy().into_owned();
    let read = conversation::read_shared(&path);
    assert_eq!(
        read.facts.model, None,
        "the cleared transcript names no model"
    );
    assert!(
        read.items.is_empty(),
        "a clear starts the conversation over"
    );
    let shown = with_model(&path, "worker", read);
    assert_eq!(shown.facts.model.as_deref(), Some("claude-opus-5-5"));
}
