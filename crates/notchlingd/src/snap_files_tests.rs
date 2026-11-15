use crate::limits_ops_tests::TempDir;
use crate::snap_files::{
    backup_db, copy_quiet, copy_quiet_hooked, copy_tail, copy_tail_hooked, Copied, StageMeta,
};
use notchling_engine::cluster::id::NodeId;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::snapshot::{SetPlan, Trigger};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub(crate) const QUIET: i64 = 10_000;

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn age(path: &Path, ms_ago: u64) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_millis(ms_ago))
        .unwrap();
}

pub(crate) fn old(path: &Path) {
    age(path, 3_600_000);
}

pub(crate) fn put(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    old(path);
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) fn node() -> NodeId {
    NodeId::from_fingerprint(&"ab".repeat(32)).unwrap()
}

pub(crate) fn meta() -> StageMeta {
    StageMeta {
        seq: 7,
        term: Term { n: 3, node: node() },
        home: node(),
        created_ms: 1_700_000_000_000,
        quiet_ms: QUIET,
        now_ms: now_ms(),
    }
}

pub(crate) fn plan(id: &str) -> SetPlan {
    SetPlan {
        id: id.to_owned(),
        trigger: Trigger::Timer,
        sessions: Vec::new(),
        tails: Vec::new(),
        files: Vec::new(),
        dbs: Vec::new(),
    }
}

pub(crate) fn staged(stage: &Path, set: &str, source: &Path) -> PathBuf {
    let rel = source.strip_prefix("/").unwrap();
    stage.join(set).join("parts").join(rel)
}

pub(crate) fn make_db(path: &Path, rows: u32) -> Connection {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT, v TEXT NOT NULL);")
        .unwrap();
    let payload = "x".repeat(900);
    let tx = conn.unchecked_transaction().unwrap();
    for _ in 0..rows {
        tx.execute("INSERT INTO t (v) VALUES (?1)", [&payload])
            .unwrap();
    }
    tx.commit().unwrap();
    conn
}

pub(crate) fn count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM t", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn backup_of_a_db_being_written_is_consistent() {
    let t = TempDir::new("snap-backup");
    let src = t.path("data/cluster.db");
    let dst = t.path("out/cluster.db");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();
    let reader = make_db(&src, 3000);
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let stop = Arc::clone(&stop);
        let src = src.clone();
        std::thread::spawn(move || {
            let conn = Connection::open(&src).unwrap();
            conn.busy_timeout(Duration::from_secs(5)).unwrap();
            let payload = "y".repeat(900);
            while !stop.load(Ordering::SeqCst) {
                conn.execute("INSERT INTO t (v) VALUES (?1)", [&payload])
                    .unwrap();
            }
        })
    };
    std::thread::sleep(Duration::from_millis(50));
    let before = count(&reader);
    let result = backup_db(&src, &dst);
    let after = count(&reader);
    stop.store(true, Ordering::SeqCst);
    writer.join().unwrap();
    assert!(count(&reader) > before);
    result.unwrap();
    let copy = Connection::open(&dst).unwrap();
    let check: String = copy
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(check, "ok");
    let rows = count(&copy);
    assert!(
        rows >= before && rows <= after,
        "{before} <= {rows} <= {after}"
    );
}

#[test]
fn backup_leaves_one_private_file() {
    let t = TempDir::new("snap-backup-one");
    let src = t.path("data/cluster.db");
    let dst = t.path("out/cluster.db");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();
    let _keep = make_db(&src, 50);
    backup_db(&src, &dst).unwrap();
    let names: Vec<String> = fs::read_dir(dst.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["cluster.db".to_owned()]);
    assert_eq!(
        fs::metadata(&dst).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn backup_of_a_missing_db_is_an_error() {
    let t = TempDir::new("snap-backup-missing");
    fs::create_dir_all(t.path("out")).unwrap();
    let result = backup_db(&t.path("nope.db"), &t.path("out/nope.db"));
    assert!(result.is_err());
    assert!(!t.path("nope.db").exists());
}

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
