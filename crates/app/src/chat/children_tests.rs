use super::data::DataRoot;
use crate::notch::source::{now_ms, DataRoot as NotchRoot, Source};
use crate::testutil::Scratch;
use std::path::Path;

const PARENT: &str = "codex:01a1d5da-0000-7000-8000-00000000000a";

fn root_with_children() -> Scratch {
    let scratch = Scratch::new("codex-children");
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../engine/tests/fixtures/codex-children/sessions/2026/11/13");
    for entry in std::fs::read_dir(from)
        .expect("the fixture folder")
        .flatten()
    {
        let text = std::fs::read_to_string(entry.path()).expect("a fixture rollout");
        scratch.write(
            &format!(
                "codex/sessions/2026/11/13/{}",
                entry.file_name().to_string_lossy()
            ),
            &text,
        );
    }
    scratch
}

#[test]
fn a_codex_child_is_not_a_sidebar_row() {
    let scratch = root_with_children();
    let rows = DataRoot::fixture(scratch.path()).sessions(now_ms());
    let ids: Vec<&str> = rows.iter().map(|row| row.agent.id.as_str()).collect();
    assert_eq!(ids, [PARENT]);
}

#[test]
fn a_codex_child_is_not_a_notch_row() {
    let scratch = root_with_children();
    let mut source = Source::keeping_everything(NotchRoot::fixture(scratch.path()));
    let loaded = source.load(now_ms());
    let ids: Vec<&str> = loaded.agents.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, [PARENT]);
}

#[test]
fn the_parent_keeps_its_own_title_and_folder() {
    let scratch = root_with_children();
    let rows = DataRoot::fixture(scratch.path()).sessions(now_ms());
    let parent = rows.first().expect("the parent");
    assert_eq!(parent.agent.name, "Plan the zebrafish migration");
    assert_eq!(parent.agent.cwd, "/work/app");
}
