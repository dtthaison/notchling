use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use notchling_local_api::ClientError;
use serde_json::{json, Value};

use super::*;
use crate::testutil::Scratch;

const PARENT: &str = "01a1d5da-0000-7000-8000-00000000000a";
const CHILD: &str = "01a1d5da-0000-7000-8000-00000000000b";

fn rollout(thread: &str, parent: Option<&str>) -> String {
    let source = parent
        .map(|p| format!(r#","parent_thread_id":"{p}","thread_source":"subagent""#))
        .unwrap_or_default();
    format!(
        "{{\"timestamp\":\"2026-11-13T09:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread}\",\"cwd\":\"/w\"{source}}}}}\n"
    )
}

fn world(s: &Scratch, parent_day: &str, child_day: &str) -> PathBuf {
    let parent = s.write(
        &format!(
            "sessions/2026/11/{parent_day}/rollout-2026-11-{parent_day}T23-59-00-{PARENT}.jsonl"
        ),
        &rollout(PARENT, None),
    );
    s.write(
        &format!("sessions/2026/11/{child_day}/rollout-2026-11-{child_day}T00-01-00-{CHILD}.jsonl"),
        &rollout(CHILD, Some(PARENT)),
    );
    parent
}

struct Down(Arc<AtomicUsize>);

impl Transport for Down {
    fn call(&self, _op: &str, _args: Value) -> Result<Value, ClientError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(ClientError::Closed)
    }
}

struct Says(Value);

impl Transport for Says {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        assert_eq!(op, OP);
        assert_eq!(args["thread"], PARENT);
        Ok(self.0.clone())
    }
}

#[test]
fn no_daemon_reads_two_day_folders() {
    let s = Scratch::new("kids-fallback");
    let parent = world(&s, "13", "14");
    let calls = Arc::new(AtomicUsize::new(0));
    let source = DaemonChildren::new(Box::new(Down(calls.clone())));
    let found = source.children(PARENT, &parent);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].thread, CHILD);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_third_day_is_not_read_without_a_daemon() {
    let s = Scratch::new("kids-third");
    let parent = world(&s, "13", "15");
    let source = DaemonChildren::new(Box::new(Down(Arc::default())));
    assert!(source.children(PARENT, &parent).is_empty());
}

#[test]
fn a_failed_daemon_is_not_asked_again_for_a_while() {
    let s = Scratch::new("kids-skip");
    let parent = world(&s, "13", "13");
    let calls = Arc::new(AtomicUsize::new(0));
    let source = DaemonChildren::new(Box::new(Down(calls.clone())));
    source.children(PARENT, &parent);
    source.children(PARENT, &parent);
    source.children(PARENT, &parent);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn the_daemons_answer_is_used_as_it_is() {
    let reply = json!({"children": [
        {"path": "/s/c.jsonl", "thread": CHILD, "parent": PARENT, "depth": 2,
         "nickname": "Euler", "role": "worker", "started": 12.5},
        {"path": "/s/bad.jsonl", "thread": ""}
    ]});
    let source = DaemonChildren::new(Box::new(Says(reply)));
    let found = source.children(PARENT, Path::new("/nowhere/x.jsonl"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].depth, 2);
    assert_eq!(found[0].nickname.as_deref(), Some("Euler"));
    assert_eq!(found[0].path, PathBuf::from("/s/c.jsonl"));
    assert_eq!(found[0].started, 12.5);
}
