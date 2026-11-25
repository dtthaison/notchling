use super::fork::own_start;
use super::testutil::{tree, FORKED, OLD_FORK, PARENT, PLAIN};
use super::*;
use crate::sessions::testutil::TempDir;
use crate::transcript::model::ItemKind;
use std::io::Write;

fn texts(path: &str) -> Vec<String> {
    read(path)
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::User { text, .. } | ItemKind::Assistant { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_top_level_rollout_starts_at_zero() {
    let dir = tree("fork-top");
    assert_eq!(own_start(&PARENT.path_in(&dir, "sessions")), 0);
    assert_eq!(own_start("/nonexistent/rollout.jsonl"), 0);
}

#[test]
fn a_child_starts_where_the_marker_says_its_own_history_does() {
    let dir = tree("fork-marker");
    assert_eq!(
        own_start(&FORKED.path_in(&dir, "sessions")),
        FORKED.offset_of_line(9)
    );
}

#[test]
fn without_the_marker_a_child_starts_at_the_task_after_the_forked_turn() {
    let dir = tree("fork-old");
    assert_eq!(
        own_start(&OLD_FORK.path_in(&dir, "sessions")),
        OLD_FORK.offset_of_line(10)
    );
}

#[test]
fn a_child_with_no_forked_block_starts_at_zero() {
    let dir = tree("fork-plain");
    assert_eq!(own_start(&PLAIN.path_in(&dir, "sessions")), 0);
}

#[test]
fn a_childs_own_turn_that_is_not_written_yet_shows_nothing_of_the_fork() {
    let dir = TempDir::new("fork-early");
    let written = FORKED.up_to_line(8);
    let path = dir
        .write("rollout-x.jsonl", &written)
        .to_string_lossy()
        .into_owned();
    assert_eq!(own_start(&path), written.len() as u64);
    assert!(texts(&path).is_empty());
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open");
    file.write_all(&FORKED.text.as_bytes()[written.len()..])
        .expect("append");
    assert_eq!(own_start(&path), FORKED.offset_of_line(9));
    assert_eq!(texts(&path), ["Count the narwhal tusks", "Four tusks."]);
}

#[test]
fn the_childs_conversation_has_only_its_own_turn() {
    let dir = tree("fork-conversation");
    for fixture in [&FORKED, &OLD_FORK] {
        let path = fixture.path_in(&dir, "sessions");
        assert_eq!(texts(&path), ["Count the narwhal tusks", "Four tusks."]);
        let c = read(&path);
        assert_eq!(c.facts.turns, 1, "the forked turn is not the child's");
        assert_eq!(c.facts.input_tokens, 250);
        assert_eq!(c.facts.model.as_deref(), Some("gpt-6-luna-mini"));
    }
    assert_eq!(
        texts(&PARENT.path_in(&dir, "sessions")),
        [
            "Plan the zebrafish migration",
            "I will ask a helper to look at the narwhal notes."
        ]
    );
}

#[test]
fn turn_open_ignores_the_forked_turn_aborted() {
    let dir = TempDir::new("fork-open");
    let running = FORKED.up_to_line(13);
    let path = dir
        .write("rollout-x.jsonl", &running)
        .to_string_lossy()
        .into_owned();
    assert!(turn_open(&path), "its own turn is running");
    let finished = dir
        .write("rollout-y.jsonl", FORKED.text)
        .to_string_lossy()
        .into_owned();
    assert!(!turn_open(&finished), "its own turn ended");
}

#[test]
fn a_forked_turn_that_never_ended_is_not_the_childs_open_turn() {
    let dir = TempDir::new("fork-unfinished");
    let forked: Vec<&str> = FORKED.text.split_inclusive('\n').collect();
    let mut text: String = forked[..8].concat();
    text.push_str(forked[9]);
    let path = dir
        .write("rollout-x.jsonl", &text)
        .to_string_lossy()
        .into_owned();
    assert!(
        !turn_open(&path),
        "a copy of the parent's turn in progress, then the child's settings, no turn of its own"
    );
}

#[test]
fn glance_ignores_the_forked_token_count() {
    let dir = TempDir::new("fork-glance");
    let before_own_tokens = FORKED.up_to_line(16);
    let early = dir
        .write("rollout-x.jsonl", &before_own_tokens)
        .to_string_lossy()
        .into_owned();
    assert_eq!(context_tokens(&early), None);
    assert_eq!(peek(&early, false).and_then(|p| p.context), None);
    let done = dir
        .write("rollout-y.jsonl", FORKED.text)
        .to_string_lossy()
        .into_owned();
    assert_eq!(context_tokens(&done), Some(300));
    assert_eq!(peek(&done, false).and_then(|p| p.context), Some(300));
    let recent = recent(&early).expect("a tail");
    let kinds: Vec<String> = recent
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::User { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["Count the narwhal tusks"]);
}

#[test]
fn a_window_starting_inside_the_fork_drops_the_rest_of_it() {
    let dir = TempDir::new("fork-window");
    let mut lines: Vec<String> = FORKED.text.lines().map(String::from).collect();
    lines[7] = lines[7].replace("{\\\"task\\\":\\\"narwhal\\\"}", &"x".repeat(100_000));
    assert!(lines[7].len() > 100_000, "the forked spawn call is big");
    let path = dir
        .write("rollout-x.jsonl", &(lines.join("\n") + "\n"))
        .to_string_lossy()
        .into_owned();
    let tail = recent(&path).expect("a tail");
    let kinds: Vec<String> = tail
        .items
        .iter()
        .map(|item| match &item.kind {
            ItemKind::User { text, .. } | ItemKind::Assistant { text } => text.clone(),
            ItemKind::Notice { .. } => "notice".into(),
            _ => "other".into(),
        })
        .collect();
    assert!(
        !kinds.iter().any(|k| k == "notice"),
        "the forked turn_aborted is not shown: {kinds:?}"
    );
    assert_eq!(
        kinds.first().map(String::as_str),
        Some("Count the narwhal tusks")
    );
    assert_eq!(context_tokens(&path), Some(300));
}
