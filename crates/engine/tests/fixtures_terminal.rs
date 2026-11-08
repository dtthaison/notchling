//! The recorded terminal bytes, loaded by Rust. Today they only have to exist and be readable; the
//! emulator port (E3) replays them into the Rust screen and asserts what the Swift tests assert
//! (`TerminalScreenTests`, `AskDialogTests`). New recordings from `tools/record-reply-fixtures.sh` land
//! in the same folder and are covered here without a change.

use std::fs;
use std::path::PathBuf;

fn folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../legacy/macos/Tests/NotchlingTests/Fixtures/terminal")
}

fn recordings() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(folder())
        .expect("the terminal fixtures folder exists")
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "bin"))
        .collect();
    found.sort();
    found
}

#[test]
fn there_are_recordings() {
    assert!(
        recordings().len() >= 6,
        "the plugin phases and question-open are the floor"
    );
}

#[test]
fn every_recording_is_non_empty_terminal_output() {
    for path in recordings() {
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(!bytes.is_empty(), "{} is empty", path.display());
        assert!(
            bytes.contains(&0x1b),
            "{} has no escape sequence, so it is not claude attach output",
            path.display()
        );
    }
}

/// A recording made by the recorder has a `.json` beside it saying where each phase starts; it has to
/// agree with the bytes, since the replay harness slices by those offsets.
#[test]
fn recorder_descriptions_match_their_bytes() {
    for path in recordings() {
        let described = path.with_extension("json");
        if !described.exists() {
            continue; // the older recordings (plugin-*, question-open) predate the recorder
        }
        let bytes = fs::read(&path).expect("readable recording");
        let text = fs::read_to_string(&described)
            .unwrap_or_else(|e| panic!("{}: {e}", described.display()));
        let meta: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", described.display()));
        assert_eq!(
            meta["byteCount"].as_u64(),
            Some(bytes.len() as u64),
            "{}",
            described.display()
        );
        let mut last = 0u64;
        for phase in meta["phases"].as_array().expect("phases is a list") {
            let offset = phase["offset"].as_u64().expect("a phase has an offset");
            assert!(
                offset >= last && offset <= bytes.len() as u64,
                "{} phase offsets",
                described.display()
            );
            last = offset;
        }
        assert!(
            meta["steps"].is_array(),
            "{} lists the keys sent",
            described.display()
        );
    }
}
