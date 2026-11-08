use super::*;
use rusqlite::Connection;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-agy-sum-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// 2026-11-08 00:00:00 UTC.
const NOW: f64 = 1_794_182_400.0;

/// A database like Antigravity's: WAL mode, with the rows still in the WAL (no checkpoint).
fn antigravity_db(root: &Path) -> Connection {
    let db = Connection::open(root.join(DB_NAME)).unwrap();
    db.pragma_update(None, "journal_mode", "WAL").unwrap();
    db.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    db.execute_batch(
        "CREATE TABLE conversation_summaries (conversation_id TEXT, title TEXT, preview TEXT, \
         workspace_uris TEXT, status TEXT, last_modified_time TEXT, nesting_depth INTEGER);",
    )
    .unwrap();
    db
}

/// One row of the table, column by column.
#[allow(clippy::too_many_arguments)]
fn insert(
    db: &Connection,
    id: &str,
    title: &str,
    preview: &str,
    uris: &str,
    status: &str,
    at: &str,
    depth: i64,
) {
    db.execute(
        "INSERT INTO conversation_summaries VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![id, title, preview, uris, status, at, depth],
    )
    .unwrap();
}

#[test]
fn it_reads_recent_top_level_rows_from_a_copy_with_its_wal() {
    let root = scratch("root");
    let copies = scratch("copy");
    let db = antigravity_db(&root);
    insert(
        &db,
        "new",
        "Fix the build",
        "",
        "[\"file:///Users/me/My%20App\"]",
        "IDLE",
        "2026-11-07 10:00:00.123456+00:00",
        0,
    );
    insert(
        &db,
        "old",
        "Ancient",
        "",
        "[]",
        "IDLE",
        "2026-10-01 10:00:00.000000+00:00",
        0,
    );
    insert(
        &db,
        "sub",
        "A subagent",
        "",
        "[]",
        "IDLE",
        "2026-11-07 11:00:00.000000+00:00",
        1,
    );
    insert(
        &db,
        "prev",
        "",
        "first words",
        "[]",
        "RUNNING",
        "2026-11-07 12:00:00.000000+00:00",
        0,
    );
    insert(
        &db,
        "none",
        "",
        "",
        "not json",
        "IDLE",
        "2026-11-06 12:00:00.000000+00:00",
        0,
    );
    insert(&db, "bad-date", "x", "", "[]", "IDLE", "yesterday", 0);
    assert!(
        root.join("conversation_summaries.db-wal").is_file(),
        "the rows are in the WAL"
    );

    let found = rows(&root, &copies.join("agy"), NOW);
    let ids: Vec<&str> = found.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids,
        ["prev", "new", "none"],
        "newest first; old, nested and undated rows left out"
    );
    assert_eq!(found[0].title, "first words");
    assert!(found[0].running);
    assert_eq!(found[0].folder, "");
    assert_eq!(found[1].folder, "/Users/me/My App");
    assert_eq!(found[1].row_id(), "agy:new");
    assert_eq!(found[2].title, "Antigravity Session");
    let foreign = found[1].to_foreign(true);
    assert_eq!(foreign.project, "My App");
    assert!(foreign.working);
    assert_eq!(foreign.since_ms, found[1].modified * 1000.0);

    // Antigravity's own files were only copied from, never changed or opened by us.
    drop(db);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&copies);
}

#[test]
fn the_copy_is_made_fresh_each_time_and_a_missing_database_is_empty() {
    let root = scratch("fresh-root");
    let copies = scratch("fresh-copy");
    assert!(rows(&root, &copies, NOW).is_empty(), "no database");
    let db = antigravity_db(&root);
    insert(
        &db,
        "a",
        "One",
        "",
        "[]",
        "IDLE",
        "2026-11-07 10:00:00.000000+00:00",
        0,
    );
    assert_eq!(rows(&root, &copies, NOW).len(), 1);
    insert(
        &db,
        "b",
        "Two",
        "",
        "[]",
        "IDLE",
        "2026-11-07 11:00:00.000000+00:00",
        0,
    );
    assert_eq!(
        rows(&root, &copies, NOW).len(),
        2,
        "the second read copies again"
    );
    // A file that isn't a database gives nothing and doesn't panic.
    let junk = scratch("junk-root");
    std::fs::write(junk.join(DB_NAME), b"not a database at all").unwrap();
    assert!(rows(&junk, &copies, NOW).is_empty());
    for d in [&root, &copies, &junk] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn dates_folders_and_titles() {
    assert_eq!(
        date("2026-11-05 16:57:03.335872+00:00"),
        Some(1_793_897_823.0)
    );
    assert_eq!(date("2026-11-05 16:57:03"), Some(1_793_897_823.0));
    assert_eq!(date("nope"), None);
    assert_eq!(folder("[\"file:///a/b\",\"file:///c\"]"), "/a/b");
    assert_eq!(folder("[\"https://x\"]"), "");
    assert_eq!(folder("[]"), "");
    assert_eq!(folder("[\"file:///a%2"), "");
    assert_eq!(folder("[\"file:///a%41%\"]"), "/aA%");
    assert_eq!(title("one\ntwo"), "one");
    assert_eq!(title(&"x".repeat(80)).chars().count(), 58);
    assert!(title(&"x".repeat(80)).ends_with('…'));
}
