use super::{month_end_ms, month_name, month_of, month_start_ms, parse_month_name, AuditDir};
use crate::sessions::testutil::TempDir;
use std::fs;

#[test]
fn month_of_and_names() {
    assert_eq!(month_of(1_794_560_523_117), 202611);
    assert_eq!(month_name(202611), "2026-11");
    assert_eq!(month_name(202601), "2026-01");
    assert_eq!(parse_month_name("2026-11"), Some(202611));
    assert_eq!(parse_month_name("2026-13"), None);
    assert_eq!(parse_month_name("26-11"), None);
    assert_eq!(month_of(0), 197001);
    assert_eq!(month_of(-1), 196912);
}

#[test]
fn month_bounds_follow_the_calendar() {
    assert_eq!(month_start_ms(197001), 0);
    assert_eq!(month_end_ms(197001), 31 * 86_400_000);
    assert_eq!(month_end_ms(202612), month_start_ms(202701));
    assert_eq!(month_of(month_end_ms(202602) - 1), 202602);
    assert_eq!(month_of(month_end_ms(202602)), 202603);
}

#[cfg(unix)]
#[test]
fn append_creates_0600_in_0700_folder() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("audit-files-mode");
    let mut audit = AuditDir::new(dir.path("audit"));
    audit.ensure().expect("ensure");
    audit.append(202611, b"{\"n\":1}\n").expect("append");
    let folder = fs::metadata(dir.path("audit")).expect("meta");
    assert_eq!(folder.permissions().mode() & 0o777, 0o700);
    let file = fs::metadata(dir.path("audit/2026-11.jsonl")).expect("meta");
    assert_eq!(file.permissions().mode() & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn append_refuses_symlink() {
    let dir = TempDir::new("audit-files-link");
    let mut audit = AuditDir::new(dir.path("audit"));
    audit.ensure().expect("ensure");
    let target = dir.write("elsewhere.txt", "keep\n");
    std::os::unix::fs::symlink(&target, dir.path("audit/2026-11.jsonl")).expect("link");
    assert!(audit.append(202611, b"x\n").is_err());
    assert_eq!(fs::read_to_string(&target).expect("read"), "keep\n");
}

#[test]
fn append_keeps_adding_and_switches_months() {
    let dir = TempDir::new("audit-files-append");
    let mut audit = AuditDir::new(dir.path("audit"));
    audit.ensure().expect("ensure");
    audit.append(202610, b"a\n").expect("append");
    audit.append(202610, b"b\n").expect("append");
    audit.append(202611, b"c\n").expect("append");
    audit.flush(true).expect("flush");
    assert_eq!(audit.read(202610).expect("read"), "a\nb\n");
    assert_eq!(audit.read(202611).expect("read"), "c\n");
    assert_eq!(audit.size(), 6);
}

#[test]
fn months_ignores_other_files() {
    let dir = TempDir::new("audit-files-months");
    dir.write("audit/2026-11.jsonl", "x\n");
    dir.write("audit/2026-09.jsonl", "x\n");
    dir.write("audit/notes.txt", "x\n");
    dir.write("audit/2026-13.jsonl", "x\n");
    dir.write("audit/legacy/audit.jsonl", "x\n");
    let audit = AuditDir::new(dir.path("audit"));
    assert_eq!(audit.months().expect("months"), vec![202609, 202611]);
    assert!(AuditDir::new(dir.path("none"))
        .months()
        .expect("months")
        .is_empty());
}

#[test]
fn tail_line_reads_last_line() {
    let dir = TempDir::new("audit-files-tail");
    dir.write("audit/2026-10.jsonl", "old\n");
    dir.write("audit/2026-11.jsonl", "one\ntwo\nthree\n\n");
    let audit = AuditDir::new(dir.path("audit"));
    assert_eq!(audit.tail_line().expect("tail"), Some("three".to_string()));
    let empty = AuditDir::new(dir.path("empty"));
    assert_eq!(empty.tail_line().expect("tail"), None);
}

#[test]
fn tail_line_of_a_long_file_is_whole() {
    let dir = TempDir::new("audit-files-long");
    let mut body = String::new();
    for n in 0..5000 {
        body.push_str(&format!("{{\"n\":{n},\"pad\":\"{}\"}}\n", "x".repeat(40)));
    }
    dir.write("audit/2026-11.jsonl", &body);
    let audit = AuditDir::new(dir.path("audit"));
    let line = audit.tail_line().expect("tail").expect("line");
    assert!(line.starts_with("{\"n\":4999,"));
    assert!(line.ends_with('}'));
}

#[test]
fn remove_drops_the_file_and_the_handle() {
    let dir = TempDir::new("audit-files-remove");
    let mut audit = AuditDir::new(dir.path("audit"));
    audit.ensure().expect("ensure");
    audit.append(202611, b"a\n").expect("append");
    audit.remove(202611).expect("remove");
    assert!(audit.months().expect("months").is_empty());
    audit.append(202611, b"b\n").expect("append");
    assert_eq!(audit.read(202611).expect("read"), "b\n");
}
