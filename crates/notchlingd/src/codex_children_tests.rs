use crate::codex_children::CodexChildren;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use notchling_local_api::{ops, Caller, ClientKind, OpSpec, Tier};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

const PARENT: &str = "01a1d5da-0000-7000-8000-00000000000a";
const CHILD: &str = "01a1d5da-0000-7000-8000-00000000000b";

fn app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

fn rollout(thread: &str, parent: Option<&str>) -> String {
    let source = match parent {
        Some(p) => format!(
            r#","parent_thread_id":"{p}","thread_source":"subagent","agent_nickname":"Euler","agent_role":"worker","source":{{"subagent":{{"thread_spawn":{{"parent_thread_id":"{p}","depth":1}}}}}}"#
        ),
        None => String::new(),
    };
    format!(
        "{{\"timestamp\":\"2026-11-13T09:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread}\",\"cwd\":\"/w\"{source}}}}}\n"
    )
}

fn write(dir: &TempDir, thread: &str, parent: Option<&str>) {
    dir.write(
        &format!("sessions/2026/11/13/rollout-2026-11-13T09-00-00-{thread}.jsonl"),
        &rollout(thread, parent),
    );
}

fn rig(tag: &str) -> (TempDir, Arc<CodexChildren>) {
    let dir = TempDir::new(tag);
    let kept = Arc::new(CodexChildren::new(
        dir.path("sessions"),
        dir.path("daemon/codex-children.db"),
    ));
    (dir, kept)
}

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn asked(kept: &CodexChildren, thread: &str) -> Vec<serde_json::Value> {
    kept.call(&app(), ops::CODEX_CHILDREN, &json!({"thread": thread}))
        .unwrap()["children"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn children_are_answered_for_a_thread() {
    let (dir, kept) = rig("children-answer");
    write(&dir, PARENT, None);
    write(&dir, CHILD, Some(PARENT));
    kept.start();
    until("the backfill", || !asked(&kept, PARENT).is_empty());
    let rows = asked(&kept, PARENT);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["thread"], CHILD);
    assert_eq!(rows[0]["parent"], PARENT);
    assert_eq!(rows[0]["depth"], 1);
    assert_eq!(rows[0]["nickname"], "Euler");
    assert_eq!(rows[0]["role"], "worker");
    assert!(rows[0]["path"].as_str().unwrap().ends_with(".jsonl"));
    assert!(asked(&kept, "nobody").is_empty());
    assert!(asked(&kept, "").is_empty());
    assert!(kept.is_child(CHILD) && !kept.is_child(PARENT));
    kept.close();
}

#[test]
fn the_op_is_app_tier_and_quiet() {
    let (_dir, kept) = rig("children-spec");
    let spec = kept.spec(ops::CODEX_CHILDREN).unwrap();
    assert_eq!(spec, OpSpec::APP_QUIET);
    assert_eq!(spec.tier, Tier::App);
    assert!(!spec.approval);
    assert_eq!(kept.spec(ops::CODEX_SEND), None);
    assert!(
        asked(&kept, PARENT).is_empty(),
        "before start nothing is known"
    );
}

#[test]
fn a_rollout_written_after_start_is_found() {
    let (dir, kept) = rig("children-late");
    write(&dir, PARENT, None);
    kept.start();
    until("the db", || dir.path("daemon/codex-children.db").is_file());
    std::thread::sleep(Duration::from_millis(500));
    write(&dir, CHILD, Some(PARENT));
    until("the watch", || !asked(&kept, PARENT).is_empty());
    kept.close();
}

#[test]
fn a_missing_db_folder_is_made() {
    let (dir, kept) = rig("children-folder");
    assert!(!dir.path("daemon").exists());
    kept.start();
    until("the db", || dir.path("daemon/codex-children.db").is_file());
    kept.close();
}
