use super::*;
use rusqlite::Connection;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-agy-kids-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const COLUMNS: &str = "conversation_id TEXT, title TEXT, preview TEXT, workspace_uris TEXT, \
     status TEXT, last_modified_time TEXT, nesting_depth INTEGER, \
     parent_conversation_id TEXT, agent_name TEXT, killed INTEGER";

fn db(dir: &Path, columns: &str) -> Connection {
    let db = Connection::open(dir.join(COPY_NAME)).unwrap();
    db.execute_batch(&format!("CREATE TABLE conversation_summaries ({columns});"))
        .unwrap();
    db
}

fn child(db: &Connection, id: &str, parent: &str, name: &str, status: &str, at: &str, depth: i64) {
    db.execute(
        "INSERT INTO conversation_summaries VALUES (?1, '', '', '[]', ?2, ?3, ?4, ?5, ?6, 0)",
        rusqlite::params![id, status, at, depth, parent, name],
    )
    .unwrap();
}

#[test]
fn children_of_a_conversation() {
    let dir = scratch("of");
    let db = db(&dir, COLUMNS);
    child(
        &db,
        "late",
        "top",
        "research",
        "CASCADE_RUN_STATUS_IDLE",
        "2026-11-14 10:05:00.5+00:00",
        1,
    );
    child(
        &db,
        "early",
        "top",
        "self",
        "CASCADE_RUN_STATUS_RUNNING",
        "2026-11-14 10:01:00.5+00:00",
        1,
    );
    child(
        &db,
        "other",
        "else",
        "self",
        "CASCADE_RUN_STATUS_IDLE",
        "2026-11-14 10:02:00.5+00:00",
        1,
    );
    db.execute(
        "UPDATE conversation_summaries SET killed = 1 WHERE conversation_id = 'late'",
        [],
    )
    .unwrap();
    let found = children(&dir.join(COPY_NAME), "top");
    let seen: Vec<_> = found
        .iter()
        .map(|c| (c.id.as_str(), c.agent_name.as_str(), c.killed, c.depth))
        .collect();
    assert_eq!(
        seen,
        [("early", "self", false, 1), ("late", "research", true, 1)]
    );
    assert!(found[0].status.contains("RUNNING"));
    assert!(found[1].modified > found[0].modified);
}

#[test]
fn a_child_of_a_child_is_found_by_its_own_id() {
    let dir = scratch("deep");
    let db = db(&dir, COLUMNS);
    child(
        &db,
        "one",
        "top",
        "self",
        "IDLE",
        "2026-11-14 10:01:00+00:00",
        1,
    );
    child(
        &db,
        "two",
        "one",
        "self",
        "IDLE",
        "2026-11-14 10:02:00+00:00",
        2,
    );
    let copy = dir.join(COPY_NAME);
    assert_eq!(children(&copy, "top").len(), 1);
    let below = children(&copy, "one");
    assert_eq!((below[0].id.as_str(), below[0].depth), ("two", 2));
    assert!(children(&copy, "two").is_empty());
}

#[test]
fn an_old_db_without_the_column_answers_none() {
    let dir = scratch("old");
    let _db = db(
        &dir,
        "conversation_id TEXT, title TEXT, preview TEXT, workspace_uris TEXT, \
         status TEXT, last_modified_time TEXT, nesting_depth INTEGER",
    );
    assert!(children(&dir.join(COPY_NAME), "top").is_empty());
    assert!(children(&dir.join("missing.db"), "top").is_empty());
}

#[test]
fn children_are_read_from_a_fresh_copy_of_the_source() {
    let root = scratch("src");
    let copies = scratch("dst");
    let source = Connection::open(root.join(DB_NAME)).unwrap();
    source
        .execute_batch(&format!("CREATE TABLE conversation_summaries ({COLUMNS});"))
        .unwrap();
    child(
        &source,
        "c",
        "top",
        "self",
        "IDLE",
        "2026-11-14 10:01:00+00:00",
        1,
    );
    drop(source);
    let found = children_cached(&root, &copies, Duration::ZERO, "top");
    assert_eq!(found.len(), 1);
    assert!(copy_file(&copies).is_file());
    assert!(children_cached(&scratch("none"), &copies, Duration::ZERO, "top").is_empty());
}
