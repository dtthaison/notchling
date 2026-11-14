use super::{has_legacy, move_legacy, AuditDir};
use crate::sessions::testutil::TempDir;
use sha2::{Digest, Sha256};
use std::fs;

#[test]
fn legacy_moves_both_files_and_counts_lines() {
    let dir = TempDir::new("legacy-both");
    dir.write("audit.jsonl", "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n");
    dir.write("audit.jsonl.1", "{\"b\":1}\n");
    let audit = AuditDir::new(dir.path("audit"));
    assert!(!has_legacy(&audit));
    let moved = move_legacy(dir.root(), &audit).expect("moves");
    assert_eq!(moved.len(), 2);
    assert_eq!(moved[0].name, "audit.jsonl");
    assert_eq!(moved[0].lines, 3);
    assert_eq!(moved[1].name, "audit.jsonl.1");
    assert_eq!(moved[1].lines, 1);
    let expected = notchling_events::hex_lower(&Sha256::digest(b"{\"b\":1}\n"));
    assert_eq!(moved[1].sha256.as_str(), expected);
    assert!(!dir.path("audit.jsonl").exists());
    assert!(dir.path("audit/legacy/audit.jsonl").exists());
    assert!(has_legacy(&audit));
}

#[test]
fn legacy_move_twice_is_noop() {
    let dir = TempDir::new("legacy-twice");
    dir.write("audit.jsonl", "{\"a\":1}\n");
    let audit = AuditDir::new(dir.path("audit"));
    assert_eq!(move_legacy(dir.root(), &audit).expect("moves").len(), 1);
    assert!(move_legacy(dir.root(), &audit).expect("moves").is_empty());
    assert_eq!(
        fs::read_to_string(dir.path("audit/legacy/audit.jsonl")).expect("read"),
        "{\"a\":1}\n"
    );
}

#[test]
fn nothing_to_move_makes_no_folder() {
    let dir = TempDir::new("legacy-none");
    let audit = AuditDir::new(dir.path("audit"));
    assert!(move_legacy(dir.root(), &audit).expect("moves").is_empty());
    assert!(!dir.path("audit").exists());
    assert!(!has_legacy(&audit));
}

#[test]
fn a_second_old_file_does_not_overwrite_the_first() {
    let dir = TempDir::new("legacy-collide");
    let audit = AuditDir::new(dir.path("audit"));
    dir.write("audit.jsonl", "first\n");
    move_legacy(dir.root(), &audit).expect("moves");
    dir.write("audit.jsonl", "second\n");
    let moved = move_legacy(dir.root(), &audit).expect("moves");
    assert_eq!(moved[0].name, "audit.jsonl.1");
    assert_eq!(
        fs::read_to_string(dir.path("audit/legacy/audit.jsonl")).expect("read"),
        "first\n"
    );
}

#[cfg(unix)]
#[test]
fn legacy_files_are_read_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("legacy-mode");
    dir.write("audit.jsonl", "x\n");
    let audit = AuditDir::new(dir.path("audit"));
    move_legacy(dir.root(), &audit).expect("moves");
    let mode = fs::metadata(dir.path("audit/legacy/audit.jsonl"))
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o400);
}
