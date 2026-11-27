use crate::acting_ops_tests::FakeRoom;
use crate::chat_ops::ChatOps;
use crate::consent_testkit::agent_caller;
use crate::op_group::OpGroup;
use notchling_local_api::ErrorCode;
use notchling_local_api::{ops, topics, OpSpec, Tier};
use serde_json::json;
use std::sync::Arc;

fn ops_with_room() -> (ChatOps, Arc<FakeRoom>) {
    let room = Arc::new(FakeRoom::default());
    (ChatOps::new(room.clone()), room)
}

fn file() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-chat-ops-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("order.rb");
    std::fs::write(&path, "puts 1\n").unwrap();
    path
}

#[test]
fn an_existing_file_is_published_to_the_app() {
    let (ops, room) = ops_with_room();
    let path = file();
    let reply = ops
        .call(
            &agent_caller(),
            ops::CHAT_OPEN_FILE,
            &json!({"path": path, "session": "s1"}),
        )
        .unwrap();
    assert_eq!(reply["ok"], json!(true));
    let events = room.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, topics::CHAT_OPEN_FILE);
    assert_eq!(events[0].1["session"], json!("s1"));
}

#[test]
fn a_missing_file_a_folder_and_a_relative_path_are_refused() {
    let (ops, room) = ops_with_room();
    let dir = file().parent().unwrap().to_path_buf();
    for path in [
        json!("/nonexistent/nl/none.rb"),
        json!(dir),
        json!("order.rb"),
        json!(7),
    ] {
        let error = ops
            .call(&agent_caller(), ops::CHAT_OPEN_FILE, &json!({"path": path}))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::BadRequest);
    }
    assert!(room.events.lock().unwrap().is_empty());
}

#[test]
fn the_op_acts_and_the_topic_is_for_the_app() {
    let (ops, _) = ops_with_room();
    assert_eq!(ops.spec(ops::CHAT_OPEN_FILE), Some(OpSpec::AGENT_ACTS));
    assert_eq!(ops.spec(ops::SHELLS_STOP), None);
    assert_eq!(ops.topic_tier(topics::CHAT), Some(Tier::App));
}

#[test]
fn a_call_forwarded_from_another_node_is_refused() {
    let (ops, room) = ops_with_room();
    let mut caller = agent_caller();
    caller.client = notchling_local_api::ClientKind::Node;
    let error = ops
        .call(&caller, ops::CHAT_OPEN_FILE, &json!({"path": file()}))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert!(room.events.lock().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn a_link_is_followed_to_its_file_and_a_pipe_or_device_is_refused() {
    let (ops, room) = ops_with_room();
    let real = file();
    let link = real.parent().unwrap().join("link.rb");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let reply = ops
        .call(&agent_caller(), ops::CHAT_OPEN_FILE, &json!({"path": link}))
        .unwrap();
    let shown = std::fs::canonicalize(&real).unwrap();
    assert_eq!(reply["path"], json!(shown));
    assert_eq!(room.events.lock().unwrap()[0].1["path"], json!(shown));
    let pipe = real.parent().unwrap().join("pipe.rb");
    let _ = std::fs::remove_file(&pipe);
    let made = std::process::Command::new("mkfifo").arg(&pipe).status();
    assert!(made.is_ok_and(|m| m.success()));
    for path in [pipe, "/dev/null".into()] {
        let error = ops
            .call(&agent_caller(), ops::CHAT_OPEN_FILE, &json!({"path": path}))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::BadRequest);
    }
}
