use super::reference::Book;
use super::soak::{Soak, Soaks};
use super::{check, Tool, Version};
use crate::sessions::testutil::TempDir;
use crate::transcript::strict::{SkipKind, Skips};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn skips() -> Skips {
    let mut skips = Skips::default();
    for _ in 0..14 {
        skips.turn(Some("2.1.324"));
    }
    for _ in 0..3 {
        skips.turn(Some("2.1.323"));
    }
    skips.note(Some("2.1.323"), SkipKind::UnknownBlock);
    skips
}

#[test]
fn counts_only_the_new_version() {
    let mut soak = Soak::new(v("2.1.324"));
    soak.observe(&skips());
    assert_eq!(soak.turns, 14);
    assert_eq!(soak.unknown, 0);
    let mut more = skips();
    more.note(Some("2.1.324"), SkipKind::UnknownLine);
    soak.observe(&more);
    assert_eq!(soak.unknown, 1);
}

#[test]
fn a_pass_that_sees_less_never_lowers_the_count() {
    let mut soak = Soak::new(v("2.1.324"));
    soak.observe(&skips());
    soak.observe(&Skips::default());
    assert_eq!(soak.turns, 14);
}

#[test]
fn line_reads_plainly() {
    let mut soak = Soak::new(v("2.1.324"));
    soak.turns = 14;
    assert_eq!(soak.line(), "Since 2.1.324: 14 turns read, nothing unknown");
    assert!(!soak.red());
    soak.unknown = 2;
    assert_eq!(soak.line(), "Since 2.1.324: 14 turns read, 2 unknown lines");
    assert!(soak.red());
    soak.turns = 1;
    soak.unknown = 1;
    assert_eq!(soak.line(), "Since 2.1.324: 1 turn read, 1 unknown line");
}

#[test]
fn new_reference_restarts_the_soak() {
    let mut soaks = Soaks::new();
    soaks
        .follow(Tool::Claude, Some(&v("2.1.324")))
        .unwrap()
        .observe(&skips());
    assert_eq!(soaks.get(Tool::Claude).unwrap().turns, 14);
    soaks.follow(Tool::Claude, Some(&v("2.1.324")));
    assert_eq!(soaks.get(Tool::Claude).unwrap().turns, 14);
    soaks.follow(Tool::Claude, Some(&v("2.1.325")));
    assert_eq!(soaks.get(Tool::Claude).unwrap().turns, 0);
    assert_eq!(soaks.get(Tool::Claude).unwrap().since, v("2.1.325"));
    assert!(soaks.follow(Tool::Claude, None).is_none());
    assert!(soaks.get(Tool::Claude).is_none());
}

#[test]
fn soak_never_changes_the_reference() {
    let mut book = Book::new();
    let outcome = check::Outcome {
        result: check::CheckResult::PartlyChecked,
        failed_part: None,
        why: None,
        unknown_count: 0,
        unchecked: check::UNCHECKED.to_vec(),
        duration_ms: 1,
    };
    book.apply(Tool::Claude, &v("2.1.324"), "aa", &outcome, 5);
    let before = book.clone();
    let mut many = Skips::default();
    for _ in 0..50 {
        many.note(Some("2.1.324"), SkipKind::UnknownLine);
    }
    let mut soak = Soak::new(v("2.1.324"));
    soak.observe(&many);
    assert_eq!(soak.unknown, 50);
    assert_eq!(book, before);
    assert!(book.usable(Tool::Claude, &v("2.1.324"), "aa"));
}

#[test]
fn round_trips_through_the_store() {
    let dir = TempDir::new("versions-soak");
    let mut soaks = Soaks::new();
    soaks
        .follow(Tool::Codex, Some(&v("0.166.0")))
        .unwrap()
        .observe(&skips());
    soaks.save(dir.root()).unwrap();
    assert_eq!(Soaks::load(dir.root()), soaks);
}
