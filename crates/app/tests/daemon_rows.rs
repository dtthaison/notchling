//! Rows from the daemon (Milestone 2): Chat and the notch show the list they are given in place of
//! reading the jobs and sessions folders, and still read transcripts and Codex rollouts
//! themselves. Over the checked-in fixture, on every system; no daemon is involved.

use std::path::{Path, PathBuf};

use notchling_app::chat::data::{DataRoot, Loader, Tool};
use notchling_app::notch::source::DataRoot as NotchRoot;
use notchling_app::notch::source::Source;

/// Late 2026-11-08, the fixture's day.
const NOW_MS: i64 = 1_794_182_400_000;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/chat")
}

#[test]
fn chat_lists_the_daemons_claude_rows_and_its_own_codex_rows() {
    let root = DataRoot::fixture(&fixture_root());
    let mut from_files = root.sessions(NOW_MS);
    let mut row = from_files
        .iter()
        .find(|s| s.tool == Tool::Claude)
        .map(|s| s.agent.clone())
        .expect("a Claude Code row in the fixture");
    row.id = "from-the-daemon".into();
    row.name = "the daemon's row".into();

    let mut loader = Loader::new(root);
    let loaded = loader.load_from(None, Some(vec![row]), NOW_MS);
    let claude: Vec<&str> = loaded
        .sessions
        .iter()
        .filter(|s| s.tool == Tool::Claude)
        .map(|s| s.agent.name.as_str())
        .collect();
    assert_eq!(claude, ["the daemon's row"], "not the folders' rows");
    assert!(
        loaded.sessions.iter().any(|s| s.tool == Tool::Codex),
        "Codex rows are still read here"
    );
    assert_eq!(loaded.selected.as_deref(), Some("from-the-daemon"));

    // With no daemon rows the folders are read, as in Milestone 1.
    from_files.sort_by(|a, b| a.agent.id.cmp(&b.agent.id));
    let again = loader.load_from(None, None, NOW_MS);
    assert_eq!(again.sessions.len(), from_files.len());
}

#[test]
fn the_notch_shows_the_daemons_rows_and_reads_their_transcripts() {
    let root = fixture_root();
    let mut source = Source::keeping_everything(NotchRoot::fixture(&root));
    let files = source.load(NOW_MS);
    let working = files
        .agents
        .iter()
        .find(|a| files.snapshots.contains_key(&a.id))
        .cloned()
        .expect("a row with a transcript");

    // Codex and Antigravity rows are read here whatever the daemon says, and whether one shows
    // depends on its file's age (a checkout's mtime), so only Claude Code's rows are compared.
    let claude = |agents: &[notchling_engine::sessions::agent::Agent]| {
        agents
            .iter()
            .filter(|a| !a.id.starts_with("codex:") && !a.id.starts_with("agy:"))
            .cloned()
            .collect::<Vec<_>>()
    };
    let loaded = source.load_from(Some(vec![working.clone()]), None, NOW_MS);
    assert_eq!(claude(&loaded.agents), std::slice::from_ref(&working));
    assert!(loaded.snapshots.contains_key(&working.id));
    assert!(claude(&loaded.agents).len() < claude(&files.agents).len());

    let empty = source.load_from(Some(Vec::new()), None, NOW_MS);
    assert!(claude(&empty.agents).is_empty());
    assert!(empty
        .snapshots
        .keys()
        .all(|id| id.starts_with("codex:") || id.starts_with("agy:")));
}
