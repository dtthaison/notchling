use crate::limits_ops_tests::TempDir;
use crate::snap_db::{backup_db, backup_db_limited, backup_db_report, BackupLimits, BackupPath};
use crate::snap_files::StageMeta;
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

fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

#[test]
fn idle_backup_leaves_the_source_untouched() {
    let t = TempDir::new("snap-backup-idle");
    let src = t.path("data/cluster.db");
    let dst = t.path("out/cluster.db");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();
    drop(make_db(&src, 200));
    old(&src);
    let bytes = fs::read(&src).unwrap();
    let stamp = mtime(&src);
    backup_db(&src, &dst).unwrap();
    assert_eq!(fs::read(&src).unwrap(), bytes);
    assert_eq!(mtime(&src), stamp);
}

#[test]
fn a_tiny_deadline_forces_one_whole_copy() {
    let t = TempDir::new("snap-backup-whole");
    let src = t.path("data/cluster.db");
    let dst = t.path("out/cluster.db");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();
    drop(make_db(&src, 500));
    let limits = BackupLimits {
        stepped_for: Duration::ZERO,
        total: Duration::from_secs(30),
    };
    assert_eq!(
        backup_db_limited(&src, &dst, &limits).unwrap(),
        BackupPath::Whole
    );
    let copy = Connection::open(&dst).unwrap();
    assert_eq!(count(&copy), 500);
    let dst2 = t.path("out/second.db");
    assert_eq!(backup_db_report(&src, &dst2).unwrap(), BackupPath::Stepped);
}

#[test]
fn a_symlinked_source_db_is_refused() {
    let t = TempDir::new("snap-backup-link");
    let src = t.path("data/cluster.db");
    let link = t.path("data/link.db");
    let dst = t.path("out/link.db");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();
    drop(make_db(&src, 5));
    std::os::unix::fs::symlink(&src, &link).unwrap();
    assert!(backup_db(&link, &dst).is_err());
    assert!(!dst.exists());
}
