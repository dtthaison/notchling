use std::path::{Path, PathBuf};

use notchling_engine::transcript::{conversation, snapshot};

fn transcripts(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            transcripts(&path, out);
        } else if path.extension().is_some_and(|e| e == "jsonl")
            && path.to_string_lossy().contains("/claude/projects/")
        {
            out.push(path);
        }
    }
}

#[test]
fn the_conversation_and_the_snapshot_agree_on_every_fixture() {
    let mut files = Vec::new();
    transcripts(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures"),
        &mut files,
    );
    assert!(files.len() >= 10, "found only {files:?}");
    let release = files
        .iter()
        .any(|p| p.ends_with("notch/claude/projects/-work-site/sess-release.jsonl"));
    assert!(release, "sess-release.jsonl is among them");

    let mut asking = 0;
    for path in &files {
        let path = path.to_string_lossy();
        let from_conversation = conversation::read(&path).pending_question;
        let from_snapshot = snapshot::read(&path).question;
        assert_eq!(from_conversation, from_snapshot, "{path}");
        asking += usize::from(from_snapshot.is_some());
    }
    assert!(asking >= 1, "no fixture holds an open question to compare");
}
