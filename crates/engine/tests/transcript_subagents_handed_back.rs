mod common;

use common::*;
use notchling_engine::transcript::subagents::{self, Status};
use notchling_engine::transcript::tail::RAW_BYTES;
use notchling_engine::transcript::util::now_secs;
use std::collections::HashMap;
use std::sync::atomic::Ordering;

#[test]
fn a_hand_back_is_read_once_per_file_stamp_and_again_when_the_file_grows() {
    let scratch = Scratch::new("handed-back");
    let transcript = s(&scratch.write("sess.jsonl", ""));
    scratch.write(
        "sess/subagents/agent-ax.meta.json",
        r#"{"description":"Port it"}"#,
    );
    let agent = scratch.write("sess/subagents/agent-ax.jsonl", "{}\n");
    let now = now_secs();
    let status = || subagents::list(&transcript, &HashMap::new(), now)[0].status;
    let read = || RAW_BYTES.load(Ordering::Relaxed);

    let before = read();
    assert_eq!(status(), Status::Running);
    let first = read() - before;
    assert!(first > 0, "the first listing reads the tail");

    assert_eq!(status(), Status::Running);
    assert_eq!(status(), Status::Running);
    assert_eq!(
        read() - before,
        first,
        "an unchanged file is not read again"
    );

    std::fs::write(&agent, "{}\nReport delivered to your caller\n").expect("grow");
    assert_eq!(status(), Status::Completed);
    assert!(read() - before > first, "a grown file is read again");

    let settled = read();
    assert_eq!(status(), Status::Completed);
    assert_eq!(read(), settled, "and then remembered");
}
