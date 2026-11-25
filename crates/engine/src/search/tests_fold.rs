use super::fts::SearchDb;
use super::{highlights, rank, snippet, IndexedSession, SessionMeta, DEFAULT_RADIUS};
use crate::sessions::testutil::TempDir;
use rusqlite::Connection;

const NOW: f64 = 2_000_000_000.0;

fn session(key: &str, title: &str, text: &str) -> IndexedSession {
    IndexedSession::new(
        SessionMeta {
            key: key.into(),
            tool_key: "claude".into(),
            session_id: key.into(),
            title: title.into(),
            folder: "/w".into(),
            path: String::new(),
            config_dir: None,
            modified: NOW,
            size: 0,
            parent: None,
            account: None,
            started: None,
        },
        text.into(),
    )
}

fn found(db: &SearchDb, words: &[&str]) -> Vec<String> {
    db.rank(words, NOW, 10)
        .into_iter()
        .map(|(k, _)| k)
        .collect()
}

#[test]
fn a_stored_dang_nhap_is_found_by_unaccented_words() {
    let db = SearchDb::memory().unwrap();
    db.put(&[session(
        "v",
        "Sửa lỗi đăng nhập",
        "Lỗi đăng nhập khi mở app",
    )])
    .unwrap();
    assert_eq!(found(&db, &["dang", "nhap"]), ["v"]);
    assert_eq!(found(&db, &["đăng"]), ["v"]);
    assert_eq!(found(&db, &["DANG"]), ["v"]);
    assert_eq!(found(&db, &["Đăng", "NHẬP"]), ["v"]);
    assert_eq!(found(&db, &["đa\u{0306}ng"]), ["v"]);
    assert_eq!(found(&db, &["dan"]), ["v"], "a prefix");
    assert!(found(&db, &["nhapp"]).is_empty());
}

#[test]
fn the_text_alone_is_found_by_folded_words() {
    let db = SearchDb::memory().unwrap();
    db.put(&[session("v", "title", "Lỗi đăng nhập khi mở app")])
        .unwrap();
    assert_eq!(found(&db, &["dang"]), ["v"]);
    assert_eq!(found(&db, &["khi", "mo"]), ["v"]);
}

#[test]
fn sessions_keep_the_original_text() {
    let tmp = TempDir::new("fold-original");
    let path = tmp.path("search.db");
    {
        let db = SearchDb::open(&path).unwrap();
        db.put(&[session("v", "Sửa lỗi đăng nhập", "Lỗi đăng nhập")])
            .unwrap();
        assert!(db.sessions()[0].text.contains("đăng"));
    }
    let again = SearchDb::open(&path).unwrap().sessions();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].text, "Lỗi đăng nhập");
    assert_eq!(again[0].meta.title, "Sửa lỗi đăng nhập");
}

#[test]
fn highlights_mark_the_accented_original() {
    let text = "Sửa lỗi đăng nhập";
    let start = text.find("đăng").unwrap();
    assert_eq!(
        highlights(text, &["dang"]),
        vec![start..start + "đăng".len()]
    );
    let word = text.find("nhập").unwrap();
    assert_eq!(
        highlights(text, &["đăng", "nhap"]),
        vec![start..start + "đăng".len(), word..text.len()]
    );
    assert_eq!(highlights("ĐĂNG", &["đang"]), vec![0.."ĐĂNG".len()]);
}

#[test]
fn the_substring_fallback_folds() {
    let sessions = vec![session("v", "t", "đăng nhập")];
    let hits = rank(&["dang"], &sessions, NOW, 10);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].meta.key, "v");
    let hits = rank(&["đăng"], &sessions, NOW, 10);
    assert_eq!(hits.len(), 1);
}

#[test]
fn a_snippet_shows_the_original_and_its_highlight_lands_on_it() {
    let text = "Hôm nay tôi sửa lỗi đăng nhập của ứng dụng và thử lại nhiều lần";
    let s = session("v", "t", text);
    let snip = snippet(&s, &["dang", "nhap"], DEFAULT_RADIUS).unwrap();
    assert!(snip.contains("đăng nhập"), "{snip}");
    let marks = highlights(&snip, &["dang", "nhap"]);
    let marked: Vec<&str> = marks.iter().map(|r| &snip[r.clone()]).collect();
    assert_eq!(marked, ["đăng", "nhập"]);
}

fn schema_2_file(path: &std::path::Path) {
    let db = Connection::open(path).unwrap();
    db.execute_batch(
        "CREATE TABLE docs (id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE,
                            modified REAL NOT NULL, meta TEXT NOT NULL);
         CREATE VIRTUAL TABLE fts USING fts5(
             title, folder, body, tokenize = 'unicode61 remove_diacritics 2');
         INSERT INTO docs (id, key, modified, meta) VALUES (1, 'old', 1.0, '{}');
         INSERT INTO fts (rowid, title, folder, body) VALUES (1, 'old', '/w', 'text');
         PRAGMA user_version = 2;",
    )
    .unwrap();
}

#[test]
fn a_schema_2_file_is_rebuilt_and_keeps_originals() {
    let tmp = TempDir::new("fold-schema-2");
    let path = tmp.path("search.db");
    schema_2_file(&path);
    {
        let db = SearchDb::open(&path).unwrap();
        assert!(db.sessions().is_empty());
        db.put(&[session("v", "Sửa lỗi đăng nhập", "Lỗi đăng nhập")])
            .unwrap();
        assert_eq!(found(&db, &["dang"]), ["v"]);
    }
    let again = SearchDb::open(&path).unwrap();
    let all = again.sessions();
    assert_eq!(all.len(), 1);
    assert!(all[0].text.contains("đăng"));
    assert_eq!(found(&again, &["dang", "nhap"]), ["v"]);
    let raw = Connection::open(&path).unwrap();
    let version: i64 = raw
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 4);
}

#[test]
fn docs_have_a_corpus_column_that_defaults_to_sessions() {
    let tmp = TempDir::new("fold-corpus");
    let path = tmp.path("search.db");
    schema_2_file(&path);
    let db = SearchDb::open(&path).unwrap();
    db.put(&[session("v", "title", "text")]).unwrap();
    let raw = Connection::open(&path).unwrap();
    let corpus: String = raw
        .query_row("SELECT corpus FROM docs WHERE key = 'v'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(corpus, "sessions");
    raw.execute(
        "INSERT INTO docs (key, modified, meta, body) VALUES ('p', 1.0, '{}', 'x')",
        [],
    )
    .unwrap();
    let default: String = raw
        .query_row("SELECT corpus FROM docs WHERE key = 'p'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(default, "sessions");
}

#[test]
fn rows_of_another_corpus_are_not_sessions() {
    let tmp = TempDir::new("fold-other-corpus");
    let path = tmp.path("search.db");
    let db = SearchDb::open(&path).unwrap();
    db.put(&[session("v", "deploy", "text")]).unwrap();
    {
        let raw = Connection::open(&path).unwrap();
        raw.execute(
            "INSERT INTO docs (id, key, modified, meta, body, corpus)
             VALUES (99, 'page', 1.0, '{}', 'deploy', 'pages')",
            [],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO fts (rowid, title, folder, body) VALUES (99, 'deploy', '', 'deploy')",
            [],
        )
        .unwrap();
    }
    assert_eq!(found(&db, &["deploy"]), ["v"]);
    assert_eq!(db.sessions().len(), 1);
}
