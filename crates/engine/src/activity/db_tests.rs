use super::db::{EventsDb, Opened};
use super::row::{ActivityRow, DbQuery};
use crate::sessions::testutil::TempDir;

fn row(seq: u64, ts: i64, kind: &str, grp: &str) -> ActivityRow {
    let mut row = ActivityRow::blank(seq, ts, kind);
    row.grp = grp.to_owned();
    row.host = "3f9a0c1b7d2e4a56".to_owned();
    row.expires = i64::MAX;
    row
}

fn query() -> DbQuery {
    DbQuery {
        limit: 100,
        ..DbQuery::default()
    }
}

fn open(dir: &TempDir) -> EventsDb {
    EventsDb::open(&dir.path("daemon/events.db")).expect("opens")
}

#[test]
fn insert_and_list_newest_first() {
    let dir = TempDir::new("events-db-list");
    let mut db = open(&dir);
    assert_eq!(db.opened(), Opened::Created);
    db.insert(&[
        row(1, 1_000, "api.refused", "security"),
        row(2, 3_000, "switch.changed", "security"),
        row(3, 2_000, "vault.secret_used", "secrets"),
    ])
    .expect("inserts");
    let rows = db.list(&query()).expect("lists");
    let seqs: Vec<u64> = rows.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, vec![2, 3, 1]);
    assert_eq!(rows[0].kind, "switch.changed");
    assert_eq!(db.max_seq(), 3);
    assert_eq!(db.count(), 3);
    let mut limited = query();
    limited.limit = 2;
    assert_eq!(db.list(&limited).expect("lists").len(), 2);
}

#[test]
fn every_column_round_trips() {
    let dir = TempDir::new("events-db-columns");
    let mut db = open(&dir);
    let mut full = row(9, 5_000, "vault.secret_used", "secrets");
    full.actor_tier = Some("agent".into());
    full.actor_via = Some("cli".into());
    full.actor_exe = Some("notchling".into());
    full.actor_session = Some("sess-1".into());
    full.session = Some("sess-1".into());
    full.folder = Some("/tmp/app".into());
    full.account = Some("acct".into());
    full.device = Some("dev".into());
    full.op_id = Some("9f2c41d07ab3e512".into());
    full.audit_n = Some(4812);
    full.spooled = true;
    full.legacy = true;
    full.f = "{\"secrets\":[\"gh\"]}".into();
    full.expires = 77;
    db.insert(std::slice::from_ref(&full)).expect("inserts");
    assert_eq!(db.list(&query()).expect("lists"), vec![full.clone()]);
    assert_eq!(db.row_with_audit_n(4812), Some(full));
}

#[test]
fn list_filters_by_group_kind_session_and_before_seq() {
    let dir = TempDir::new("events-db-filters");
    let mut db = open(&dir);
    let mut a = row(1, 1_000, "api.refused", "security");
    a.session = Some("s1".into());
    let b = row(2, 2_000, "vault.secret_used", "secrets");
    let mut c = row(3, 3_000, "session.reply", "agents");
    c.session = Some("s1".into());
    db.insert(&[a, b, c]).expect("inserts");
    let mut q = query();
    q.groups = vec!["secrets".into(), "agents".into()];
    assert_eq!(db.list(&q).expect("lists").len(), 2);
    q.kinds = vec!["session.reply".into()];
    assert_eq!(db.list(&q).expect("lists").len(), 1);
    let mut q = query();
    q.session = Some("s1".into());
    assert_eq!(db.list(&q).expect("lists").len(), 2);
    let mut q = query();
    q.before_seq = Some(3);
    let seqs: Vec<u64> = db.list(&q).expect("lists").iter().map(|r| r.seq).collect();
    assert_eq!(seqs, vec![2, 1]);
    let mut q = query();
    q.since = Some(1_500);
    assert_eq!(db.list(&q).expect("lists").len(), 2);
}

#[test]
fn prune_removes_expired_only() {
    let dir = TempDir::new("events-db-prune");
    let mut db = open(&dir);
    let mut old = row(1, 1_000, "api.refused", "security");
    old.expires = 5_000;
    let mut fresh = row(2, 2_000, "api.refused", "security");
    fresh.expires = 50_000;
    db.insert(&[old, fresh]).expect("inserts");
    assert_eq!(db.prune(10_000).expect("prunes"), 1);
    let rows = db.list(&query()).expect("lists");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].seq, 2);
}

#[test]
fn trim_to_drops_oldest_first() {
    let dir = TempDir::new("events-db-trim");
    let mut db = open(&dir);
    let rows: Vec<ActivityRow> = (1..=3000u64)
        .map(|n| {
            let mut r = row(n, n as i64, "api.refused", "security");
            r.f = format!("{{\"pad\":\"{}\"}}", "x".repeat(400));
            r
        })
        .collect();
    db.insert(&rows).expect("inserts");
    let before = db.size_bytes();
    let removed = db.trim_to(before / 2).expect("trims");
    assert!(removed > 0);
    let left = db.list(&query()).expect("lists");
    assert!(!left.is_empty());
    assert!(left.iter().all(|r| r.seq > removed), "oldest went first");
    assert_eq!(db.max_seq(), 3000);
    assert!(db.size_bytes() <= before / 2 + 8192);
}

#[test]
fn clear_keeps_locked_kinds_unless_asked() {
    let dir = TempDir::new("events-db-clear");
    let mut db = open(&dir);
    db.insert(&[
        row(1, 1, "api.refused", "security"),
        row(2, 2, "op.called", "security"),
        row(3, 3, "session.reply", "agents"),
    ])
    .expect("inserts");
    assert_eq!(db.clear(&[], false).expect("clears"), 0);
    let security = vec!["security".to_owned()];
    let removed = db.clear(&security, false).expect("clears");
    let left: Vec<String> = db
        .list(&query())
        .expect("lists")
        .into_iter()
        .map(|r| r.kind)
        .collect();
    assert_eq!(removed, 1, "{left:?}");
    assert!(left.contains(&"api.refused".to_string()));
    assert_eq!(db.clear(&security, true).expect("clears"), 1);
}

#[test]
fn meta_round_trip() {
    let dir = TempDir::new("events-db-meta");
    let mut db = open(&dir);
    assert_eq!(db.meta_get("imported"), None);
    db.meta_set("imported", "1").expect("sets");
    db.meta_set("imported", "2").expect("sets");
    assert_eq!(db.meta_get("imported"), Some("2".into()));
    drop(db);
    let again = open(&dir);
    assert_eq!(again.opened(), Opened::Existing);
    assert_eq!(again.meta_get("imported"), Some("2".into()));
}

#[test]
fn damaged_db_is_set_aside() {
    let dir = TempDir::new("events-db-damaged");
    let junk = vec![0x5a_u8; 100];
    std::fs::create_dir_all(dir.path("daemon")).expect("mkdir");
    std::fs::write(dir.path("daemon/events.db"), &junk).expect("writes");
    let mut db = open(&dir);
    assert_eq!(db.opened(), Opened::Rebuilt);
    assert_eq!(
        std::fs::read(dir.path("daemon/events.db.damaged")).expect("aside"),
        junk
    );
    db.insert(&[row(1, 1, "api.refused", "security")])
        .expect("inserts");
    assert_eq!(db.count(), 1);
}

#[cfg(unix)]
#[test]
fn symlink_db_is_refused() {
    let dir = TempDir::new("events-db-link");
    let target = dir.write("elsewhere.db", "keep");
    std::fs::create_dir_all(dir.path("daemon")).expect("mkdir");
    std::os::unix::fs::symlink(&target, dir.path("daemon/events.db")).expect("link");
    assert!(EventsDb::open(&dir.path("daemon/events.db")).is_err());
    assert_eq!(std::fs::read_to_string(&target).expect("read"), "keep");
}

#[cfg(unix)]
#[test]
fn file_is_0600() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("events-db-mode");
    let _db = open(&dir);
    let mode = std::fs::metadata(dir.path("daemon/events.db"))
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}
