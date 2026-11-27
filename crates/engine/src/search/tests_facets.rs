use super::fts::{FtsDoc, SearchDb};
use super::query::filter::Filter;
use super::{IndexedSession, SessionMeta};
use crate::sessions::testutil::TempDir;
use rusqlite::Connection;

const NOW: f64 = 2_000_000_000.0;

fn session(
    key: &str,
    tool: &str,
    folder: &str,
    account: &str,
    text: &str,
    age: f64,
) -> IndexedSession {
    IndexedSession::new(
        SessionMeta {
            key: key.into(),
            tool_key: tool.into(),
            session_id: key.into(),
            title: key.into(),
            folder: folder.into(),
            path: String::new(),
            config_dir: None,
            modified: NOW - age,
            size: 0,
            parent: None,
            account: Some(account.into()),
            started: None,
        },
        text.into(),
    )
}

fn only(tool: &str) -> Filter {
    Filter {
        tool: Some(vec![tool.into()]),
        ..Filter::default()
    }
}

fn keys(hits: &[(String, f64)]) -> Vec<&str> {
    hits.iter().map(|(k, _)| k.as_str()).collect()
}

#[test]
fn a_filter_applies_inside_fts() {
    let db = SearchDb::memory().unwrap();
    let mut all: Vec<IndexedSession> = (0..60)
        .map(|i| {
            session(
                &format!("c{i}"),
                "claude",
                "/w/a",
                "work",
                "alpha alpha alpha alpha",
                10.0 + f64::from(i),
            )
        })
        .collect();
    all.push(session("x", "codex", "/w/b", "home", "alpha", 90_000.0));
    db.put(&all).unwrap();
    let plain = db.rank(&["alpha"], NOW, 10);
    assert!(!keys(&plain).contains(&"x"));
    let hits = db.rank_filtered(&["alpha"], &only("codex"), NOW, 10);
    assert_eq!(keys(&hits), ["x"]);
}

#[test]
fn project_account_and_time_filters_combine() {
    let db = SearchDb::memory().unwrap();
    db.put(&[
        session("a", "claude", "/w/a", "work", "alpha", 100.0),
        session("b", "claude", "/w/b", "work", "alpha", 100.0),
        session("c", "claude", "/w/a", "home", "alpha", 100.0),
        session("d", "claude", "/w/a", "work", "alpha", 90_000.0),
    ])
    .unwrap();
    let filter = Filter {
        project: Some(vec!["/w/a".into()]),
        account: Some(vec!["work".into()]),
        after: Some(NOW - 3600.0),
        ..Filter::default()
    };
    assert_eq!(keys(&db.rank_filtered(&["alpha"], &filter, NOW, 10)), ["a"]);
    let none = Filter {
        owner: Some(vec!["work".into()]),
        ..Filter::default()
    };
    assert!(db.rank_filtered(&["alpha"], &none, NOW, 10).is_empty());
}

#[test]
fn a_removed_doc_leaves_no_facets() {
    let db = SearchDb::memory().unwrap();
    db.put(&[session("a", "codex", "/w/a", "work", "alpha", 1.0)])
        .unwrap();
    db.remove(&["a"]).unwrap();
    db.put(&[session("b", "claude", "/w/a", "work", "alpha", 1.0)])
        .unwrap();
    assert!(db
        .rank_filtered(&["alpha"], &only("codex"), NOW, 10)
        .is_empty());
    assert_eq!(db.facet_rows(), 3);
}

#[test]
fn page_docs_carry_their_facets() {
    let db = SearchDb::memory().unwrap();
    db.put_docs(
        "pages",
        &[FtsDoc {
            key: "page:a".into(),
            title: "Plan".into(),
            folder: "app".into(),
            body: "alpha".into(),
            at: NOW,
            facets: vec![
                ("owner".into(), "work".into()),
                ("project".into(), "app".into()),
            ],
        }],
    )
    .unwrap();
    let filter = Filter {
        owner: Some(vec!["work".into()]),
        ..Filter::default()
    };
    assert_eq!(
        keys(&db.rank_corpus_filtered("pages", &["alpha"], &filter, NOW, 10)),
        ["page:a"]
    );
    assert!(db.rank_filtered(&["alpha"], &filter, NOW, 10).is_empty());
}

#[test]
fn a_schema_3_file_is_rebuilt() {
    let tmp = TempDir::new("facets-schema-3");
    let path = tmp.path("search.db");
    {
        let raw = Connection::open(&path).unwrap();
        raw.execute_batch(
            "CREATE TABLE docs (id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE,
                                modified REAL NOT NULL, meta TEXT NOT NULL, body TEXT NOT NULL,
                                corpus TEXT NOT NULL DEFAULT 'sessions');
             INSERT INTO docs (id, key, modified, meta, body) VALUES (1, 'old', 1.0, '{}', 'x');
             PRAGMA user_version = 3;",
        )
        .unwrap();
    }
    let db = SearchDb::open(&path).unwrap();
    assert!(db.sessions().is_empty());
    db.put(&[session("a", "codex", "/w", "work", "alpha", 1.0)])
        .unwrap();
    assert_eq!(
        keys(&db.rank_filtered(&["alpha"], &only("codex"), NOW, 5)),
        ["a"]
    );
}
