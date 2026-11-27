use super::*;
use crate::limits_ops_tests::TempDir;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;

fn open(path: &Path) -> File {
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .unwrap()
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_log_over_the_cap_moves_to_dot_one_and_the_same_descriptor_writes_to_a_new_file() {
    let dir = TempDir::new("logrot");
    let path = dir.root().join("notchlingd.log");
    let mut file = open(&path);
    let fd = file.as_raw_fd();
    let log = StderrLog::adopt_fds(&path, 100, &[fd], fd).unwrap();
    file.write_all(&[b'a'; 60]).unwrap();
    assert!(!log.check().unwrap());
    assert_eq!(names(dir.root()), ["notchlingd.log"]);
    file.write_all(&[b'b'; 60]).unwrap();
    assert!(log.check().unwrap());
    file.write_all(b"after\n").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "after\n");
    assert_eq!(fs::read(old_path(&path)).unwrap().len(), 120);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn a_second_rotation_replaces_the_old_copy_so_only_one_is_kept() {
    let dir = TempDir::new("logrot-twice");
    let path = dir.root().join("notchlingd.log");
    let mut file = open(&path);
    let fd = file.as_raw_fd();
    let log = StderrLog::adopt_fds(&path, 10, &[fd], fd).unwrap();
    file.write_all(b"first-generation").unwrap();
    assert!(log.check().unwrap());
    file.write_all(b"second-generation").unwrap();
    assert!(log.check().unwrap());
    file.write_all(b"third").unwrap();
    assert_eq!(names(dir.root()), ["notchlingd.log", "notchlingd.log.1"]);
    assert_eq!(
        fs::read_to_string(old_path(&path)).unwrap(),
        "second-generation"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "third");
}

#[test]
fn every_listed_descriptor_follows_the_new_file() {
    let dir = TempDir::new("logrot-both");
    let path = dir.root().join("notchlingd.log");
    let mut out = open(&path);
    let mut err = out.try_clone().unwrap();
    let (a, b) = (out.as_raw_fd(), err.as_raw_fd());
    let log = StderrLog::adopt_fds(&path, 5, &[a, b], b).unwrap();
    out.write_all(b"0123456789").unwrap();
    assert!(log.check().unwrap());
    out.write_all(b"o").unwrap();
    err.write_all(b"e").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "oe");
}

#[test]
fn a_descriptor_that_is_not_the_log_is_left_alone() {
    let dir = TempDir::new("logrot-other");
    let path = dir.root().join("notchlingd.log");
    let _log = open(&path);
    let other = open(&dir.root().join("other.txt"));
    let fd = other.as_raw_fd();
    assert!(StderrLog::adopt_fds(&path, 1, &[fd], fd).is_none());
    assert!(StderrLog::adopt_fds(&dir.root().join("missing.log"), 1, &[fd], fd).is_none());
}

#[test]
fn a_log_someone_deleted_is_opened_again_without_a_dot_one() {
    let dir = TempDir::new("logrot-deleted");
    let path = dir.root().join("notchlingd.log");
    let mut file = open(&path);
    let fd = file.as_raw_fd();
    let log = StderrLog::adopt_fds(&path, 5, &[fd], fd).unwrap();
    file.write_all(b"0123456789").unwrap();
    fs::remove_file(&path).unwrap();
    assert!(log.check().unwrap());
    file.write_all(b"new").unwrap();
    assert_eq!(names(dir.root()), ["notchlingd.log"]);
    assert_eq!(fs::read_to_string(&path).unwrap(), "new");
}

#[test]
fn the_cap_is_five_megabytes_and_the_old_name_adds_dot_one() {
    assert_eq!(ROTATE_AT, 5 * 1024 * 1024);
    assert_eq!(
        old_path(Path::new("/d/notchlingd.log")),
        Path::new("/d/notchlingd.log.1")
    );
}
