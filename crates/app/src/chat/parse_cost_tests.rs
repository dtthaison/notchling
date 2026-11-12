//! What Chat's transcript reads cost in parsed bytes: the bench's `parsed_bytes` counter is only
//! worth anything if an unchanged file adds nothing and an append adds only its own line.

use notchling_engine::transcript::conversation::Parser;
use notchling_engine::transcript::tail::{Cursors, CHAT_TAIL_BYTES};

use crate::testutil::Scratch;

/// Three lines of `crates/app/fixtures/chat`'s transcripts.
const THREE_LINES: &str = concat!(
    r#"{"type":"user","timestamp":"2026-11-08T08:02:00.000Z","gitBranch":"m1-chat","message":{"role":"user","content":"Port the search panel."}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2026-11-08T08:02:05.000Z","effort":"medium","message":{"id":"m1","model":"claude-sonnet-5-5","content":[{"type":"text","text":"Starting with the results list, then the hot key."},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/Users/me/work/notchling/legacy/macos/Sources/Notchling/Search/SearchPanel.swift"}}],"usage":{"input_tokens":900,"output_tokens":40}}}"#,
    "\n",
    r#"{"type":"user","timestamp":"2026-11-02T13:52:10.000Z","message":{"role":"user","content":"<bash-input>git branch</bash-input>"}}"#,
    "\n",
);

const ONE_MORE: &str = concat!(
    r#"{"type":"assistant","timestamp":"2026-11-08T08:02:09.000Z","message":{"id":"m2","model":"claude-sonnet-5-5","content":[{"type":"text","text":"Done."}],"usage":{"input_tokens":1000,"output_tokens":5}}}"#,
    "\n",
);

#[test]
fn an_unchanged_reread_parses_nothing_and_an_append_parses_only_its_line() {
    let dir = Scratch::new("parse-cost");
    let path = dir.write("session.jsonl", THREE_LINES);
    let path = path.to_string_lossy().into_owned();
    let cursors = Cursors::new(CHAT_TAIL_BYTES, 4, |p: &Parser| p.conversation());

    let first = cursors.read(&path).expect("the file reads");
    assert_eq!(cursors.parsed_bytes(), THREE_LINES.len() as u64);

    let second = cursors.read(&path).expect("the file reads again");
    assert_eq!(
        cursors.parsed_bytes(),
        THREE_LINES.len() as u64,
        "an unchanged file parses nothing"
    );
    assert_eq!(first, second);

    dir.write("session.jsonl", &format!("{THREE_LINES}{ONE_MORE}"));
    let third = cursors.read(&path).expect("the grown file reads");
    assert_eq!(
        cursors.parsed_bytes(),
        (THREE_LINES.len() + ONE_MORE.len()) as u64,
        "an append parses its line and its newline, nothing before"
    );
    assert!(third.items.len() > second.items.len());
}
