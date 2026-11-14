use super::check::{CheckPart, CheckResult, Outcome};
use super::record::{Fact, MemoryRecorder};
use super::reference::{load, save, Book, FAILED_DOC, REFERENCES_DOC};
use super::{Tool, Version};
use crate::sessions::testutil::TempDir;
use crate::store;

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn outcome(result: CheckResult, part: Option<CheckPart>) -> Outcome {
    Outcome {
        result,
        failed_part: part,
        why: part.map(|_| "fixed words".to_string()),
        unknown_count: 0,
        unchecked: vec!["screens", "dialogs", "turns"],
        duration_ms: 5,
    }
}

fn partly() -> Outcome {
    outcome(CheckResult::PartlyChecked, None)
}

fn failed() -> Outcome {
    outcome(CheckResult::Failed, Some(CheckPart::ExecFlags))
}

#[test]
fn partly_checked_becomes_the_reference() {
    let mut book = Book::new();
    book.apply(Tool::Claude, &v("2.1.323"), "aa", &partly(), 10);
    let r = book.reference(Tool::Claude).unwrap();
    assert_eq!(r.version, v("2.1.323"));
    assert_eq!(r.result, CheckResult::PartlyChecked);
    assert_eq!(r.unchecked, vec!["screens", "dialogs", "turns"]);
    assert!(book.usable(Tool::Claude, &v("2.1.323"), "aa"));
}

#[test]
fn failed_keeps_the_old_reference() {
    let mut book = Book::new();
    book.apply(Tool::Claude, &v("2.1.323"), "aa", &partly(), 10);
    book.apply(Tool::Claude, &v("2.1.324"), "bb", &failed(), 20);
    assert_eq!(book.reference(Tool::Claude).unwrap().version, v("2.1.323"));
    assert!(book.is_failed(Tool::Claude, &v("2.1.324")));
    assert!(!book.usable(Tool::Claude, &v("2.1.324"), "bb"));
    assert_eq!(
        book.failure(Tool::Claude, &v("2.1.324")).unwrap().part,
        CheckPart::ExecFlags
    );
}

#[test]
fn same_version_other_binary_is_stale() {
    let mut book = Book::new();
    book.apply(Tool::Codex, &v("0.166.0"), "aa", &partly(), 10);
    assert!(book.usable(Tool::Codex, &v("0.166.0"), "aa"));
    assert!(!book.usable(Tool::Codex, &v("0.166.0"), "other"));
    assert!(!book.usable(Tool::Codex, &v("0.166.0"), ""));
}

#[test]
fn failed_version_stays_failed_when_it_returns() {
    let mut book = Book::new();
    book.apply(Tool::Claude, &v("2.1.323"), "aa", &partly(), 10);
    book.apply(Tool::Claude, &v("2.1.324"), "bb", &failed(), 20);
    book.apply(Tool::Claude, &v("2.1.325"), "cc", &partly(), 30);
    assert!(book.is_failed(Tool::Claude, &v("2.1.324")));
    assert!(!book.usable(Tool::Claude, &v("2.1.324"), "bb"));
    assert!(book.usable(Tool::Claude, &v("2.1.325"), "cc"));
}

#[test]
fn check_again_then_pass_clears_it() {
    let mut book = Book::new();
    book.apply(Tool::Claude, &v("2.1.324"), "bb", &failed(), 20);
    book.check_again(Tool::Claude, &v("2.1.324"));
    assert!(!book.is_failed(Tool::Claude, &v("2.1.324")));
    book.apply(Tool::Claude, &v("2.1.324"), "bb", &partly(), 40);
    assert!(book.usable(Tool::Claude, &v("2.1.324"), "bb"));
    book.apply(Tool::Claude, &v("2.1.326"), "dd", &failed(), 50);
    book.apply(Tool::Claude, &v("2.1.326"), "dd", &partly(), 60);
    assert!(!book.is_failed(Tool::Claude, &v("2.1.326")));
}

#[test]
fn cant_tell_changes_nothing() {
    let mut book = Book::new();
    book.apply(Tool::Claude, &v("2.1.323"), "aa", &partly(), 10);
    let before = book.clone();
    book.apply(
        Tool::Claude,
        &v("2.1.324"),
        "bb",
        &outcome(CheckResult::CantTell, None),
        20,
    );
    assert_eq!(book, before);
}

#[test]
fn round_trips_through_the_store() {
    let dir = TempDir::new("versions-ref");
    let mut book = Book::new();
    book.apply(Tool::Claude, &v("2.1.323"), "aa", &partly(), 10);
    book.apply(Tool::Agy, &v("1.4.2"), "cc", &partly(), 11);
    book.apply(Tool::Claude, &v("2.1.324"), "bb", &failed(), 20);
    save(dir.root(), &book).unwrap();
    let recorder = MemoryRecorder::new();
    let loaded = load(dir.root(), &recorder);
    assert_eq!(loaded, book);
    assert!(recorder.facts().is_empty());
    let text = store::doc(dir.root(), REFERENCES_DOC).unwrap().unwrap();
    assert!(text.contains("\"antigravity\""), "{text}");
}

#[test]
fn bad_doc_loads_empty() {
    let dir = TempDir::new("versions-ref-bad");
    store::put_doc(dir.root(), REFERENCES_DOC, "{ not json").unwrap();
    store::put_doc(dir.root(), FAILED_DOC, "[1,2").unwrap();
    let recorder = MemoryRecorder::new();
    let book = load(dir.root(), &recorder);
    assert_eq!(book, Book::new());
    assert_eq!(
        recorder.facts(),
        vec![
            Fact::LoadFailed {
                doc: REFERENCES_DOC
            },
            Fact::LoadFailed { doc: FAILED_DOC },
        ]
    );
}

#[test]
fn a_reference_under_the_wrong_tool_key_is_dropped() {
    let dir = TempDir::new("versions-ref-key");
    let mut book = Book::new();
    book.apply(Tool::Claude, &v("2.1.323"), "aa", &partly(), 10);
    save(dir.root(), &book).unwrap();
    let text = store::doc(dir.root(), REFERENCES_DOC)
        .unwrap()
        .unwrap()
        .replace("\"claude\":{", "\"codex\":{");
    store::put_doc(dir.root(), REFERENCES_DOC, &text).unwrap();
    let loaded = load(dir.root(), &MemoryRecorder::new());
    assert!(loaded.reference(Tool::Claude).is_none());
    assert!(loaded.reference(Tool::Codex).is_none());
}
