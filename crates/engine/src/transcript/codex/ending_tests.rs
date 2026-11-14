use super::testutil::{tree, FORKED, OLD_FORK, PARENT};
use super::*;
use crate::sessions::testutil::TempDir;

fn written(dir: &TempDir, name: &str, text: &str) -> String {
    dir.write(&format!("w/{name}.jsonl"), text)
        .to_string_lossy()
        .into_owned()
}

fn line(kind: &str, payload: &str) -> String {
    format!(r#"{{"type":"event_msg","payload":{{"type":"{kind}"{payload}}}}}"#) + "\n"
}

#[test]
fn a_finished_child_is_completed_whatever_its_forked_block_ended_with() {
    let dir = tree("ending-done");
    assert_eq!(
        ending(&FORKED.path_in(&dir, "sessions")),
        Ending::Completed,
        "the fork holds a turn_aborted, the child's own turn a task_complete"
    );
    assert_eq!(
        ending(&OLD_FORK.path_in(&dir, "sessions")),
        Ending::Completed
    );
}

#[test]
fn a_child_in_its_own_turn_is_running() {
    let dir = TempDir::new("ending-running");
    let path = written(&dir, "a", &FORKED.up_to_line(13));
    assert_eq!(ending(&path), Ending::Running);
}

#[test]
fn an_aborted_own_turn_is_stopped_and_a_failed_one_is_failed() {
    let dir = TempDir::new("ending-end");
    let open = FORKED.up_to_line(13);
    let aborted = written(&dir, "b", &(open.clone() + &line("turn_aborted", "")));
    assert_eq!(ending(&aborted), Ending::Stopped);
    let errored = written(
        &dir,
        "c",
        &(open.clone() + &line("error", r#","message":"x""#)),
    );
    assert_eq!(ending(&errored), Ending::Failed);
    let failed = written(
        &dir,
        "d",
        &(open.clone() + &line("task_complete", r#","error":"boom""#)),
    );
    assert_eq!(ending(&failed), Ending::Failed);
    let clean = written(
        &dir,
        "e",
        &(open + &line("task_complete", r#","error":null"#)),
    );
    assert_eq!(ending(&clean), Ending::Completed);
}

#[test]
fn a_file_with_no_turn_says_nothing_and_a_top_level_one_reads_whole() {
    let dir = tree("ending-none");
    let empty = written(&dir, "f", "");
    assert_eq!(ending(&empty), Ending::Unknown);
    assert_eq!(ending("/nonexistent.jsonl"), Ending::Unknown);
    let parent = ending(&PARENT.path_in(&dir, "sessions"));
    assert_ne!(parent, Ending::Unknown);
}
