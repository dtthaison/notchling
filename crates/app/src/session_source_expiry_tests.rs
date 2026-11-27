use super::*;
use crate::chat::outgoing::GRACE_MS;
use notchling_engine::sessions::{AgentGroup, AgentTarget};
use serde_json::json;

const SESSION: &str = "ab12cd34";

fn agent() -> Agent {
    Agent {
        id: "6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c".into(),
        name: "main".into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job(SESSION.into()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: None,
    }
}

fn wire(id: &str) -> Value {
    json!({
        "id": id, "session": SESSION, "tool": "claude", "kind": "prompt",
        "text": "look\n\n/tmp/drops/a.png", "state": "sending", "position": 1,
        "queuedMs": 10_000, "updatedMs": 10_000, "via": "chat",
    })
}

fn source_with_sent(id: &str, sent_ms: i64) -> SessionSource {
    let source = SessionSource::files();
    {
        let mut state = source
            .shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        state.outbox.seed(&[wire(id)]);
        assert!(state.outbox.sent(id, sent_ms));
    }
    source
}

#[test]
fn a_sent_bubble_nothing_matches_leaves_once_the_grace_is_over() {
    let source = source_with_sent("o1", now_ms() - GRACE_MS - 1_000);
    let before = source.generation();
    let nothing = Conversation::default();
    source.settle_outbox_expired(&agent(), &nothing);
    assert!(source.generation() > before, "the view is told to redraw");
    assert!(source.outbox_bubbles(&agent(), &nothing).is_empty());
}

#[test]
fn a_fresh_sent_bubble_stays_and_nothing_is_redrawn() {
    let source = source_with_sent("o1", now_ms());
    let before = source.generation();
    let nothing = Conversation::default();
    source.settle_outbox_expired(&agent(), &nothing);
    assert_eq!(source.generation(), before);
    assert_eq!(source.outbox_bubbles(&agent(), &nothing).len(), 1);
}
