mod common;

use common::*;
use notchling_engine::transcript::{codex, conversation, subagents};
use std::sync::Arc;

#[test]
fn an_unreadable_path_reads_as_an_empty_shared_conversation() {
    assert!(conversation::read_shared("/nonexistent/x.jsonl")
        .items
        .is_empty());
    assert!(conversation::read_subagent_shared("/nonexistent/x.jsonl")
        .items
        .is_empty());
    assert!(codex::read_shared("/nonexistent/x.jsonl").items.is_empty());
    assert!(subagents::read_shared("/nonexistent/x.jsonl")
        .items
        .is_empty());
}

#[test]
fn read_is_the_content_of_read_shared() {
    let scratch = Scratch::new("shared");
    let line = r#"{"type":"user","timestamp":"2026-11-08T08:02:00.000Z","message":{"role":"user","content":"Hello"}}"#;
    let path = s(&scratch.write("s.jsonl", &format!("{line}\n")));
    let shared: Arc<_> = conversation::read_shared(&path);
    assert_eq!(conversation::read(&path), *shared);
    assert!(
        Arc::ptr_eq(&shared, &conversation::read_shared(&path)),
        "an unchanged file gives the same Arc"
    );
    assert_eq!(shared.items.len(), 1);
}
