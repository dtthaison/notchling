mod common;

use common::*;
use notchling_engine::transcript::subagents::{self, Status};
use notchling_engine::transcript::util::now_secs;
use std::collections::HashMap;

#[test]
fn a_hand_back_is_remembered_per_file_stamp_and_seen_again_when_the_file_grows() {
    let scratch = Scratch::new("handed-back");
    let transcript = s(&scratch.write("sess.jsonl", ""));
    scratch.write(
        "sess/subagents/agent-ax.meta.json",
        r#"{"description":"Port it"}"#,
    );
    let agent = scratch.write("sess/subagents/agent-ax.jsonl", "{}\n");
    let now = now_secs();
    let status = || subagents::list(&transcript, &HashMap::new(), now)[0].status;
    assert_eq!(status(), Status::Running);
    assert_eq!(status(), Status::Running);
    std::fs::write(&agent, "{}\nReport delivered to your caller\n").expect("grow");
    assert_eq!(status(), Status::Completed);
}
