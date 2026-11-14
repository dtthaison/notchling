use super::testutil::{tree, FORKED, OLD_FORK, PARENT, PLAIN};
use super::*;
use crate::sessions::testutil::TempDir;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{Duration, UNIX_EPOCH};

fn first_line(dir: &TempDir, name: &str, payload: &str) -> String {
    let line = format!(r#"{{"ordinal":0,"type":"session_meta","payload":{payload}}}"#);
    dir.write(name, &(line + "\n"))
        .to_string_lossy()
        .into_owned()
}

#[test]
fn the_first_session_meta_names_the_child() {
    let dir = tree("lineage-first");
    for fixture in [&FORKED, &OLD_FORK, &PLAIN] {
        let found = meta(&fixture.path_in(&dir, "sessions")).expect("meta");
        assert_eq!(found.id, fixture.thread);
        assert_eq!(found.cwd, "/work/app");
    }
}

#[test]
fn a_child_is_titled_by_its_own_task() {
    let dir = tree("lineage-title");
    let title = |f: &testutil::Fixture| meta(&f.path_in(&dir, "sessions")).and_then(|m| m.title);
    assert_eq!(title(&FORKED).as_deref(), Some("Count the narwhal tusks"));
    assert_eq!(title(&OLD_FORK).as_deref(), Some("Count the narwhal tusks"));
    assert_eq!(title(&PLAIN).as_deref(), Some("Review the plankton notes"));
    assert_eq!(
        title(&PARENT).as_deref(),
        Some("Plan the zebrafish migration")
    );
}

#[test]
fn a_top_level_rollout_has_no_lineage() {
    let dir = tree("lineage-top");
    assert_eq!(lineage(&PARENT.path_in(&dir, "sessions")), None);
}

#[test]
fn a_child_names_its_parent_and_how_it_was_spawned() {
    let dir = tree("lineage-child");
    let forked = lineage(&FORKED.path_in(&dir, "sessions")).expect("lineage");
    assert_eq!(forked.parent, PARENT.thread);
    assert_eq!(forked.depth, 1);
    assert_eq!(forked.nickname.as_deref(), Some("Euler"));
    assert_eq!(forked.role.as_deref(), Some("worker"));
    assert_eq!(forked.agent_path.as_deref(), Some("/root/euler"));
    let plain = lineage(&PLAIN.path_in(&dir, "sessions")).expect("lineage");
    assert_eq!(plain.parent, PARENT.thread);
    assert_eq!(plain.nickname.as_deref(), Some("Kepler"));
    assert_eq!(plain.role, None);
}

#[test]
fn lineage_reads_the_spawn_source_when_the_top_field_is_missing() {
    let dir = TempDir::new("lineage-spawn");
    let path = first_line(
        &dir,
        "a.jsonl",
        r#"{"id":"x1","cwd":"/w","source":{"subagent":{"thread_spawn":{"parent_thread_id":"p9","depth":2,"agent_nickname":"Ada","agent_role":"explorer","agent_path":"/root/a/b"}}}}"#,
    );
    assert_eq!(
        lineage(&path),
        Some(Lineage {
            parent: "p9".into(),
            depth: 2,
            nickname: Some("Ada".into()),
            role: Some("explorer".into()),
            agent_path: Some("/root/a/b".into()),
        })
    );
}

#[test]
fn a_subagent_thread_with_no_parent_is_a_child_of_an_unknown_one() {
    let dir = TempDir::new("lineage-orphan");
    let by_source = first_line(&dir, "a.jsonl", r#"{"id":"x2","thread_source":"subagent"}"#);
    let orphan = lineage(&by_source).expect("a child");
    assert_eq!(orphan.parent, "");
    assert_eq!(orphan.depth, 1);
    let review = first_line(
        &dir,
        "b.jsonl",
        r#"{"id":"x3","source":{"subagent":"review"}}"#,
    );
    assert_eq!(lineage(&review).map(|l| l.parent), Some(String::new()));
}

#[test]
fn a_fork_the_user_made_is_still_a_session() {
    let dir = TempDir::new("lineage-user-fork");
    let path = first_line(
        &dir,
        "a.jsonl",
        r#"{"id":"x4","forked_from_id":"p1","thread_source":"user","source":"cli"}"#,
    );
    assert_eq!(lineage(&path), None);
}

#[test]
fn a_file_with_no_first_line_has_no_lineage() {
    let dir = TempDir::new("lineage-none");
    let empty = dir.write("a.jsonl", "").to_string_lossy().into_owned();
    let half = dir
        .write("b.jsonl", r#"{"type":"session_meta","payload":{"id":"x""#)
        .to_string_lossy()
        .into_owned();
    assert_eq!(lineage(&empty), None);
    assert_eq!(lineage(&half), None);
    assert_eq!(lineage("/nonexistent/rollout.jsonl"), None);
}

#[test]
fn lineage_is_read_once_per_stamp() {
    let dir = TempDir::new("lineage-stamp");
    let child = r#"{"type":"session_meta","payload":{"id":"a","thread_source":"subagent","parent_thread_id":"p"}}"#;
    let top = r#"{"type":"session_meta","payload":{"id":"a","thread_source":"user"}}"#;
    let path = dir
        .write("a.jsonl", &format!("{child}\n"))
        .to_string_lossy()
        .into_owned();
    let stamped = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .unwrap_or(UNIX_EPOCH + Duration::from_secs(1));
    assert_eq!(lineage(&path).map(|l| l.parent), Some("p".into()));

    let padding = " ".repeat(child.len() - top.len());
    std::fs::write(&path, format!("{top}{padding}\n")).expect("rewrite");
    OpenOptions::new()
        .write(true)
        .open(&path)
        .and_then(|f| f.set_modified(stamped))
        .expect("restore the time");
    assert_eq!(
        lineage(&path).map(|l| l.parent),
        Some("p".into()),
        "same size and time: answered from what was read"
    );

    let mut file = OpenOptions::new().append(true).open(&path).expect("open");
    file.write_all(b"{}\n").expect("append");
    assert_eq!(lineage(&path), None, "it grew: read again");
}
