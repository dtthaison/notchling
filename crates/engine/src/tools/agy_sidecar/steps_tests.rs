use super::steps::{SqliteSteps, Steps};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

const ID: &str = "457369a9-35a1-40f8-92b9-26b6398b78a1";

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-steps-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn make(dir: &Path, id: &str, rows: u32) {
    let conn = Connection::open(dir.join(format!("{id}.db"))).unwrap();
    conn.execute_batch(
        "create table steps (idx integer primary key, step_type integer, status integer)",
    )
    .unwrap();
    for i in 0..rows {
        conn.execute("insert into steps values (?1, 14, 3)", [i])
            .unwrap();
    }
}

#[test]
fn the_rows_of_the_steps_table_are_counted_from_a_copy() {
    let dir = folder("count");
    make(&dir, ID, 3);
    let steps = SqliteSteps::new(dir.clone(), dir.join("scratch"));
    assert!(steps.exists(ID));
    assert_eq!(steps.count(ID), Some(3));
    let conn = Connection::open(dir.join(format!("{ID}.db"))).unwrap();
    conn.execute("insert into steps values (9, 101, 3)", [])
        .unwrap();
    assert_eq!(steps.count(ID), Some(4));
    let left: Vec<_> = std::fs::read_dir(dir.join("scratch")).unwrap().collect();
    assert!(left.is_empty(), "the copies are removed");
}

#[test]
fn a_missing_conversation_or_a_bad_id_counts_nothing() {
    let dir = folder("missing");
    let steps = SqliteSteps::new(dir.clone(), dir.join("scratch"));
    assert!(!steps.exists(ID));
    assert_eq!(steps.count(ID), None);
    assert!(!steps.exists("../etc/passwd"));
    assert_eq!(steps.count("../x"), None);
}

#[test]
fn a_file_that_is_not_a_database_counts_nothing() {
    let dir = folder("junk");
    std::fs::write(dir.join(format!("{ID}.db")), b"not a database at all").unwrap();
    let steps = SqliteSteps::new(dir.clone(), dir.join("scratch"));
    assert!(steps.exists(ID));
    assert_eq!(steps.count(ID), None);
}
