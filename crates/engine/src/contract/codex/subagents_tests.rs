use std::path::PathBuf;
use std::sync::Arc;

use super::subagents::CodexSubagents;
use crate::contract::subagents::{Roots, SessionRef, Subagents};
use crate::contract::Tool;
use crate::sessions::testutil::TempDir;
use crate::tools::codex::children::DayFolders;
use crate::transcript::codex::testutil::{tree, FORKED, PARENT};
use crate::transcript::model::ItemKind;
use crate::transcript::subagents::{Origin, Status, Subagent};
use crate::transcript::util::now_secs;

const RUNNING: &str = "01a1d5da-0000-7000-8000-0000000000e1";

fn child(dir: &TempDir, thread: &str, text: &str) -> PathBuf {
    dir.write(
        &format!("sessions/2026/11/13/rollout-2026-11-13T09-05-00-{thread}.jsonl"),
        &text.replace(FORKED.thread, thread),
    )
}

fn parent_of(dir: &TempDir) -> SessionRef {
    SessionRef {
        id: format!("codex:{}", PARENT.thread),
        tool: Tool::Codex,
        transcript: Some(PathBuf::from(PARENT.path_in(dir, "sessions"))),
        name: String::new(),
        finished: Arc::default(),
        session: format!("codex:{}", PARENT.thread),
    }
}

fn listed(dir: &TempDir) -> Vec<Subagent> {
    CodexSubagents::new(Arc::new(DayFolders))
        .list(&parent_of(dir), &Roots::default(), now_secs())
        .unwrap()
}

fn find<'a>(rows: &'a [Subagent], thread: &str) -> &'a Subagent {
    rows.iter()
        .find(|r| r.id == format!("codex:{thread}"))
        .unwrap_or_else(|| panic!("{thread} in {rows:?}"))
}

#[test]
fn two_children_with_their_statuses() {
    let dir = tree("sub-two");
    child(&dir, RUNNING, &FORKED.up_to_line(15));
    let rows = listed(&dir);
    let done = find(&rows, FORKED.thread);
    assert_eq!(done.status, Status::Completed);
    assert_eq!(done.origin, Origin::native(Tool::Codex));
    assert_eq!(done.description, "Count the narwhal tusks");
    assert_eq!(done.agent_type, "worker");
    assert!(done.started > 0.0 && done.path.ends_with(".jsonl"));
    assert_eq!(find(&rows, RUNNING).status, Status::Running);
}

#[test]
fn a_failed_child_says_failed() {
    let dir = tree("sub-failed");
    let text =
        FORKED.up_to_line(15) + "{\"type\":\"event_msg\",\"payload\":{\"type\":\"error\"}}\n";
    child(&dir, RUNNING, &text);
    assert_eq!(find(&listed(&dir), RUNNING).status, Status::Failed);
}

#[test]
fn an_aborted_child_says_stopped() {
    let dir = tree("sub-stopped");
    let text = FORKED.up_to_line(15)
        + "{\"type\":\"event_msg\",\"payload\":{\"type\":\"turn_aborted\"}}\n";
    child(&dir, RUNNING, &text);
    assert_eq!(find(&listed(&dir), RUNNING).status, Status::Stopped);
}

#[test]
fn a_silent_child_with_an_open_turn_is_quiet_after_ten_minutes() {
    let dir = tree("sub-quiet");
    child(&dir, RUNNING, &FORKED.up_to_line(15));
    let later = CodexSubagents::new(Arc::new(DayFolders))
        .list(&parent_of(&dir), &Roots::default(), now_secs() + 3_600.0)
        .unwrap();
    assert_eq!(find(&later, RUNNING).status, Status::Quiet);
}

#[test]
fn glance_has_tokens_and_a_doing_line_for_the_running_one() {
    let dir = tree("sub-glance");
    child(&dir, RUNNING, &FORKED.up_to_line(17));
    let rows = listed(&dir);
    let running = find(&rows, RUNNING);
    let glance = CodexSubagents::new(Arc::new(DayFolders))
        .glance(running)
        .unwrap();
    let tokens = glance.tokens.expect("the child's own token_count");
    assert!(tokens < 1_000, "not the forked block's: {tokens}");
    assert!(glance.recent.is_some());
}

#[test]
fn open_shows_only_the_childs_turn() {
    let dir = tree("sub-open");
    let rows = listed(&dir);
    let conversation = CodexSubagents::new(Arc::new(DayFolders))
        .open(find(&rows, FORKED.thread))
        .unwrap();
    let texts: Vec<&str> = conversation
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::User { text, .. } | ItemKind::Assistant { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts.first(), Some(&"Count the narwhal tusks"));
    assert!(texts.iter().all(|t| !t.contains("zebrafish")));
}

#[test]
fn a_session_of_another_tool_has_no_codex_children() {
    let dir = tree("sub-other");
    let mut parent = parent_of(&dir);
    parent.tool = Tool::Claude;
    let rows = CodexSubagents::new(Arc::new(DayFolders))
        .list(&parent, &Roots::default(), now_secs())
        .unwrap();
    assert!(rows.is_empty());
}
