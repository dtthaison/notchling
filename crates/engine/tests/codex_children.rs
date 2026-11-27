use notchling_engine::tools::codex::list;
use std::path::{Path, PathBuf};

const PARENT: &str = "01a1d5da-0000-7000-8000-00000000000a";
const FORKED: &str = "01a1d5da-0000-7000-8000-00000000000b";
const PLAIN: &str = "01a1d5da-0000-7000-8000-00000000000c";
const OLD_FORK: &str = "01a1d5da-0000-7000-8000-00000000000d";

fn sessions() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-children/sessions")
}

fn rollout(thread: &str) -> PathBuf {
    list::find_rollout(&sessions(), thread).expect("a fixture rollout")
}

#[test]
fn a_child_rollout_is_not_a_session() {
    for thread in [FORKED, PLAIN, OLD_FORK] {
        let path = rollout(thread);
        assert_eq!(list::session(&path), None, "{thread}");
        let (row, lineage) = list::child(&path).expect("a child");
        assert_eq!(row.id, format!("codex:{thread}"));
        assert_eq!(lineage.parent, PARENT);
        assert_eq!(lineage.depth, 1);
    }
}

#[test]
fn a_parent_is_still_a_session() {
    let path = rollout(PARENT);
    let row = list::session(&path).expect("a row");
    assert_eq!(row.id, format!("codex:{PARENT}"));
    assert_eq!(row.name, "Plan the zebrafish migration");
    assert_eq!(list::child(&path), None);
}

#[test]
fn a_day_of_rollouts_lists_only_the_parent_as_a_session() {
    let now_ms = list::days_from_civil(2026, 11, 14) * 86_400_000;
    let found = list::rollouts(&sessions(), None, now_ms, 30);
    assert_eq!(found.len(), 4, "every rollout is a file on disk");
    let rows: Vec<String> = found
        .iter()
        .filter_map(|path| list::session(path))
        .map(|row| row.id)
        .collect();
    assert_eq!(rows, [format!("codex:{PARENT}")]);
}

#[test]
fn a_child_row_is_named_by_its_own_task() {
    let (row, _) = list::child(&rollout(FORKED)).expect("a child");
    assert_eq!(row.name, "Count the narwhal tusks");
    assert_eq!(row.cwd, "/work/app");
    let (plain, lineage) = list::child(&rollout(PLAIN)).expect("a child");
    assert_eq!(plain.name, "Review the plankton notes");
    assert_eq!(lineage.nickname.as_deref(), Some("Kepler"));
}
