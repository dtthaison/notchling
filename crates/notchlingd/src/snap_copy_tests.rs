use crate::limits_ops_tests::TempDir;
use crate::snap_copy::{copy_quiet, copy_quiet_hooked, copy_tail, copy_tail_hooked, Copied};
use crate::snap_files_tests::{age, hex, now_ms, old, put, QUIET};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, SystemTime};

#[test]
fn quiet_file_is_copied_with_its_hash() {
    let t = TempDir::new("snap-quiet");
    let src = t.path("a.json");
    let dst = t.path("out.json");
    put(&src, b"{\"a\":1}");
    let copied = copy_quiet(&src, &dst, QUIET, now_ms()).unwrap();
    assert_eq!(
        copied,
        Copied::Done {
            len: 7,
            sha256: hex(b"{\"a\":1}")
        }
    );
    assert_eq!(fs::read(&dst).unwrap(), b"{\"a\":1}");
    assert_eq!(
        fs::metadata(&dst).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn busy_file_is_not_copied() {
    let t = TempDir::new("snap-busy");
    let src = t.path("a.json");
    let dst = t.path("out.json");
    put(&src, b"fresh");
    age(&src, 100);
    assert_eq!(
        copy_quiet(&src, &dst, QUIET, now_ms()).unwrap(),
        Copied::Busy
    );
    assert!(!dst.exists());
    age(&src, QUIET as u64 + 5_000);
    assert!(matches!(
        copy_quiet(&src, &dst, QUIET, now_ms()).unwrap(),
        Copied::Done { .. }
    ));
}

#[test]
fn file_changed_during_the_copy_is_busy() {
    let t = TempDir::new("snap-changed");
    let src = t.path("a.json");
    let dst = t.path("out.json");
    put(&src, b"first");
    let again = src.clone();
    let copied = copy_quiet_hooked(&src, &dst, QUIET, now_ms(), &mut || {
        fs::write(&again, b"second and longer").unwrap();
    })
    .unwrap();
    assert_eq!(copied, Copied::Busy);
    assert!(!dst.exists());
}

#[test]
fn tail_copies_only_new_bytes() {
    let t = TempDir::new("snap-tail");
    let src = t.path("t.jsonl");
    let dst = t.path("tail");
    put(&src, b"hello world");
    let copied = copy_tail(&src, 6, &dst, QUIET, now_ms()).unwrap();
    assert_eq!(
        copied,
        Copied::Done {
            len: 5,
            sha256: hex(b"world")
        }
    );
    assert_eq!(fs::read(&dst).unwrap(), b"world");
}

#[test]
fn tail_from_the_end_is_empty() {
    let t = TempDir::new("snap-tail-end");
    let src = t.path("t.jsonl");
    let dst = t.path("tail");
    put(&src, b"abc");
    let copied = copy_tail(&src, 3, &dst, QUIET, now_ms()).unwrap();
    assert_eq!(
        copied,
        Copied::Done {
            len: 0,
            sha256: hex(b"")
        }
    );
}

#[test]
fn busy_transcript_is_not_copied() {
    let t = TempDir::new("snap-tail-busy");
    let src = t.path("t.jsonl");
    let dst = t.path("tail");
    put(&src, b"abcdef");
    age(&src, 50);
    assert_eq!(
        copy_tail(&src, 2, &dst, QUIET, now_ms()).unwrap(),
        Copied::Busy
    );
    assert!(!dst.exists());
}

#[test]
fn shorter_transcript_is_rewritten() {
    let t = TempDir::new("snap-short");
    let src = t.path("t.jsonl");
    let dst = t.path("tail");
    put(&src, b"abc");
    assert_eq!(
        copy_tail(&src, 10, &dst, QUIET, now_ms()).unwrap(),
        Copied::Rewritten
    );
    assert!(!dst.exists());
}

#[test]
fn replaced_transcript_is_rewritten() {
    let t = TempDir::new("snap-replaced");
    let src = t.path("t.jsonl");
    let dst = t.path("tail");
    put(&src, b"0123456789");
    let again = src.clone();
    let copied = copy_tail_hooked(&src, 4, &dst, QUIET, now_ms(), &mut || {
        let other = again.with_file_name("other");
        fs::write(&other, b"0123456789abcdef").unwrap();
        fs::rename(&other, &again).unwrap();
    })
    .unwrap();
    assert_eq!(copied, Copied::Rewritten);
    assert!(!dst.exists());
}

fn touch_other_mtime(path: &std::path::Path) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(1800))
        .unwrap();
}

#[test]
fn a_same_length_change_with_a_new_mtime_is_busy() {
    let t = TempDir::new("snap-mtime");
    let src = t.path("a.json");
    put(&src, b"aaaa");
    let again = src.clone();
    let copied = copy_quiet_hooked(&src, &t.path("out"), QUIET, now_ms(), &mut || {
        touch_other_mtime(&again)
    })
    .unwrap();
    assert_eq!(copied, Copied::Busy);
    assert!(!t.path("out").exists());
    let copied = copy_tail_hooked(&src, 1, &t.path("tail"), QUIET, now_ms(), &mut || {
        touch_other_mtime(&again)
    })
    .unwrap();
    assert_eq!(copied, Copied::Busy);
    assert!(!t.path("tail").exists());
}

#[test]
fn an_in_place_rewrite_during_a_tail_copy_is_busy() {
    let t = TempDir::new("snap-inplace");
    let src = t.path("t.jsonl");
    let dst = t.path("tail");
    put(&src, b"0123456789");
    let again = src.clone();
    let copied = copy_tail_hooked(&src, 4, &dst, QUIET, now_ms(), &mut || {
        fs::write(&again, b"abcdefghijklmnopqrstuvwxyz").unwrap();
    })
    .unwrap();
    assert_eq!(copied, Copied::Busy);
    assert!(!dst.exists());
}

#[test]
fn a_truncation_in_place_during_a_tail_copy_is_rewritten() {
    let t = TempDir::new("snap-truncated");
    let src = t.path("t.jsonl");
    let dst = t.path("tail");
    put(&src, b"0123456789");
    let again = src.clone();
    let copied = copy_tail_hooked(&src, 4, &dst, QUIET, now_ms(), &mut || {
        fs::write(&again, b"01").unwrap();
    })
    .unwrap();
    assert_eq!(copied, Copied::Rewritten);
    assert!(!dst.exists());
}

#[test]
fn a_destination_that_was_not_made_by_the_call_is_left_alone() {
    let t = TempDir::new("snap-keep-dst");
    let src = t.path("a.json");
    let dst = t.path("existing");
    put(&src, b"abc");
    fs::write(&dst, b"keep me").unwrap();
    age(&src, 10);
    assert_eq!(
        copy_quiet(&src, &dst, QUIET, now_ms()).unwrap(),
        Copied::Busy
    );
    assert_eq!(fs::read(&dst).unwrap(), b"keep me");
    old(&src);
    assert_eq!(
        copy_tail(&src, 99, &dst, QUIET, now_ms()).unwrap(),
        Copied::Rewritten
    );
    assert_eq!(fs::read(&dst).unwrap(), b"keep me");
}

#[test]
fn a_fifo_is_refused_without_blocking() {
    let t = TempDir::new("snap-fifo");
    let fifo = t.path("pipe");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    if !made.map(|s| s.success()).unwrap_or(false) {
        return;
    }
    assert!(copy_quiet(&fifo, &t.path("out"), QUIET, now_ms()).is_err());
    assert!(copy_tail(&fifo, 0, &t.path("out2"), QUIET, now_ms()).is_err());
}
