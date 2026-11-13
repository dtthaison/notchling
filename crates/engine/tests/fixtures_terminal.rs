use notchling_engine::terminal::{ask_dialog, dialog_open, TerminalScreen};
use std::fs;
use std::path::{Path, PathBuf};

fn start_size(path: &Path, meta: Option<&serde_json::Value>) -> (usize, usize) {
    if let Some(meta) = meta {
        let resized = meta["steps"].as_array().is_some_and(|steps| {
            steps.iter().any(|s| {
                s["label"]
                    .as_str()
                    .is_some_and(|l| l.starts_with("resize "))
            })
        });
        let size =
            |r: &str, c: &str| Some((meta[r].as_u64()? as usize, meta[c].as_u64()? as usize));
        if let Some(size) = size("initialRows", "initialCols") {
            return size;
        }
        if !resized {
            if let Some(size) = size("rows", "cols") {
                return size;
            }
        }
        return (50, 200);
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with("plugin-") {
        (30, 100)
    } else {
        (50, 200)
    }
}

fn meta_of(path: &Path) -> Option<serde_json::Value> {
    let text = fs::read_to_string(path.with_extension("json")).ok()?;
    Some(serde_json::from_str(&text).expect("a recorder description is JSON"))
}

fn replay(path: &Path, bytes: &[u8], chunk: usize) -> TerminalScreen {
    let meta = meta_of(path);
    let (rows, cols) = start_size(path, meta.as_ref());
    let mut screen = TerminalScreen::new(rows, cols);
    let mut resizes: Vec<(usize, usize, usize)> = Vec::new();
    if let Some(steps) = meta.as_ref().and_then(|m| m["steps"].as_array()) {
        for step in steps {
            let Some(size) = step["label"]
                .as_str()
                .and_then(|l| l.strip_prefix("resize "))
            else {
                continue;
            };
            let (r, c) = size.split_once('x').expect("resize RxC");
            let offset = step["offset"].as_u64().expect("a step has an offset") as usize;
            resizes.push((offset, r.parse().expect("rows"), c.parse().expect("cols")));
        }
    }
    let mut at = 0;
    for (offset, r, c) in resizes {
        for piece in bytes[at..offset].chunks(chunk) {
            screen.feed(piece);
        }
        screen.resize(r, c);
        at = offset;
    }
    for piece in bytes[at..].chunks(chunk) {
        screen.feed(piece);
    }
    screen
}

#[test]
fn every_recording_replays_to_a_drawn_screen() {
    for path in recordings() {
        let bytes = fs::read(&path).expect("readable recording");
        let screen = replay(&path, &bytes, bytes.len().max(1));
        assert_eq!(screen.lines().len(), screen.rows(), "{}", path.display());
        assert!(
            screen.lines().iter().any(|l| !l.trim().is_empty()),
            "{} draws nothing",
            path.display()
        );
    }
}

#[test]
fn every_recording_draws_the_same_however_it_is_split() {
    for path in recordings() {
        let bytes = fs::read(&path).expect("readable recording");
        let whole = replay(&path, &bytes, bytes.len().max(1));
        for chunk in [1, 7, 4096] {
            let split = replay(&path, &bytes, chunk);
            assert_eq!(
                split.cells(),
                whole.cells(),
                "{} in {chunk}-byte pieces",
                path.display()
            );
        }
    }
}

#[test]
fn recorder_recordings_end_on_the_swift_screen() {
    for path in recordings() {
        let Some(meta) = meta_of(&path) else { continue };
        let bytes = fs::read(&path).expect("readable recording");
        let screen = replay(&path, &bytes, bytes.len().max(1));
        let expected: Vec<String> = meta["screenAtEnd"]
            .as_array()
            .expect("screenAtEnd is a list")
            .iter()
            .map(|l| l.as_str().expect("a line").to_string())
            .collect();
        assert_eq!(screen.lines(), expected, "{}", path.display());
    }
}

#[test]
fn the_question_recording_is_an_open_dialog_and_the_plugin_one_is_not() {
    for path in recordings() {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let bytes = fs::read(&path).expect("readable recording");
        let screen = replay(&path, &bytes, bytes.len().max(1));
        if name == "question-open.bin" {
            assert!(dialog_open(&screen), "{name}");
            let page = ask_dialog::page(&screen.lines()).expect("the dialog reads");
            assert_eq!(page.question, "Favourite color?");
        } else if name == "plugin-0-attach.bin" {
            assert!(
                !dialog_open(&screen),
                "{name}: the plain prompt takes replies"
            );
            assert_eq!(ask_dialog::page(&screen.lines()), None);
        }
    }
}

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

#[test]
fn recorder_descriptions_match_their_bytes() {
    for path in recordings() {
        let described = path.with_extension("json");
        if !described.exists() {
            continue;
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
