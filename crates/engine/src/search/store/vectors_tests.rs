use super::*;
use crate::search::chunk::{hash, ChunkKind, ChunkText};
use crate::search::corpus::Doc;
use crate::search::embed::ModelId;
use crate::sessions::testutil::TempDir;
use std::collections::{HashMap, HashSet};

fn chunk(text: &str) -> ChunkText {
    ChunkText {
        kind: ChunkKind::Turn,
        text: text.to_string(),
    }
}

fn doc(key: &str, texts: &[&str]) -> Doc {
    Doc {
        key: key.to_string(),
        title: format!("title {key}"),
        at: 100.0,
        facets: vec![
            ("tool".to_string(), "claude".to_string()),
            ("project".to_string(), "/w/app".to_string()),
        ],
        chunks: texts.iter().map(|t| chunk(t)).collect(),
    }
}

fn model(name: &str) -> ModelId {
    ModelId(name.to_string())
}

fn vec_for(seed: f32) -> Vec<f32> {
    (0..8).map(|i| seed + i as f32).collect()
}

#[test]
fn put_then_stamps_round_trip() {
    let db = VectorDb::memory().unwrap();
    db.put_doc("sessions", "s1", &doc("a", &["one", "two"]))
        .unwrap();
    db.put_doc("pages", "p1", &doc("b", &["three"])).unwrap();
    let want: HashMap<String, String> = [("a".to_string(), "s1".to_string())].into();
    assert_eq!(db.stamps("sessions"), want);
    db.put_doc("sessions", "s2", &doc("a", &["one", "two", "more"]))
        .unwrap();
    assert_eq!(db.stamps("sessions")["a"], "s2");
    let stats = db.stats();
    assert_eq!(stats.docs, 2);
    assert_eq!(stats.chunks, 4);
    assert_eq!(stats.facets, 4);
}

#[test]
fn remove_drops_chunks_and_facets() {
    let db = VectorDb::memory().unwrap();
    db.put_doc("sessions", "s1", &doc("a", &["one", "two"]))
        .unwrap();
    db.put_doc("sessions", "s1", &doc("b", &["three"])).unwrap();
    db.remove_docs("sessions", &["a".to_string()]).unwrap();
    let stats = db.stats();
    assert_eq!((stats.docs, stats.chunks, stats.facets), (1, 1, 2));
    assert!(!db.stamps("sessions").contains_key("a"));
}

#[test]
fn missing_lists_chunks_without_this_models_vector() {
    let db = VectorDb::memory().unwrap();
    let (a, b) = (model("a/p1"), model("b/p1"));
    db.put_doc("sessions", "s1", &doc("x", &["one", "two", "one"]))
        .unwrap();
    let all = db.missing(&a, 10);
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|(h, t)| *h == hash("one") && t == "one"));
    assert_eq!(db.missing(&a, 1).len(), 1);
    db.put_vectors(&a, VecFormat::Q8, &[(hash("one"), vec_for(1.0))])
        .unwrap();
    assert_eq!(db.missing(&a, 10), vec![(hash("two"), "two".to_string())]);
    assert_eq!(db.missing(&b, 10).len(), 2);
}

#[test]
fn sweep_removes_vectors_nothing_references() {
    let db = VectorDb::memory().unwrap();
    let m = model("a/p1");
    db.put_doc("sessions", "s1", &doc("x", &["one", "two"]))
        .unwrap();
    db.put_vectors(
        &m,
        VecFormat::Q8,
        &[(hash("one"), vec_for(1.0)), (hash("two"), vec_for(2.0))],
    )
    .unwrap();
    assert_eq!(db.sweep().unwrap(), 0);
    db.remove_docs("sessions", &["x".to_string()]).unwrap();
    assert_eq!(db.stats().vectors, 2);
    assert_eq!(db.sweep().unwrap(), 2);
    assert_eq!(db.stats().vectors, 0);
}

#[test]
fn a_reput_keeps_the_vectors_of_unchanged_chunks() {
    let db = VectorDb::memory().unwrap();
    let m = model("a/p1");
    db.put_doc("sessions", "s1", &doc("x", &["one", "two"]))
        .unwrap();
    db.put_vectors(
        &m,
        VecFormat::Q8,
        &[(hash("one"), vec_for(1.0)), (hash("two"), vec_for(2.0))],
    )
    .unwrap();
    db.put_doc("sessions", "s2", &doc("x", &["one", "two", "three"]))
        .unwrap();
    assert_eq!(db.sweep().unwrap(), 0);
    assert_eq!(
        db.missing(&m, 10),
        vec![(hash("three"), "three".to_string())]
    );
}

#[test]
fn drop_model_and_coverage() {
    let db = VectorDb::memory().unwrap();
    let (a, b) = (model("a/p1"), model("b/p1"));
    db.put_doc("sessions", "s1", &doc("x", &["one", "two"]))
        .unwrap();
    db.put_doc("sessions", "s1", &doc("y", &["three"])).unwrap();
    db.put_doc("pages", "p1", &doc("z", &["four"])).unwrap();
    db.put_vectors(
        &a,
        VecFormat::F16,
        &[(hash("one"), vec_for(1.0)), (hash("four"), vec_for(4.0))],
    )
    .unwrap();
    db.put_vectors(&b, VecFormat::Q8, &[(hash("one"), vec_for(1.0))])
        .unwrap();
    let mut cover = db.coverage(&a);
    cover.sort_by(|l, r| l.corpus.cmp(&r.corpus));
    assert_eq!(
        cover,
        vec![
            Coverage {
                corpus: "pages".to_string(),
                docs: 1,
                chunks: 1,
                embedded: 1
            },
            Coverage {
                corpus: "sessions".to_string(),
                docs: 2,
                chunks: 3,
                embedded: 1
            },
        ]
    );
    db.drop_model(&a).unwrap();
    assert_eq!(db.stats().vectors, 1);
    assert!(db.coverage(&a).iter().all(|c| c.embedded == 0));
    assert_eq!(db.missing(&a, 10).len(), 4);
}

#[cfg(unix)]
#[test]
fn the_file_is_private_and_a_link_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("vec-private");
    let path = dir.path("daemon/vectors.db");
    let db = VectorDb::open(&path).unwrap();
    db.put_doc("sessions", "s1", &doc("a", &["one"])).unwrap();
    drop(db);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let folder = std::fs::metadata(dir.path("daemon"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(folder, 0o700);
    let link = dir.path("daemon/link.db");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(VectorDb::open(&link).is_err());
    assert!(VectorDb::open(&path)
        .unwrap()
        .stamps("sessions")
        .contains_key("a"));
}

#[test]
fn a_damaged_file_is_set_aside() {
    let dir = TempDir::new("vec-damaged");
    let path = dir.write("daemon/vectors.db", "this is not a database at all, no");
    let db = VectorDb::open(&path).unwrap();
    assert!(
        dir.path("daemon/vectors.damaged").exists()
            || dir.path("daemon/vectors.db.damaged").exists()
    );
    db.put_doc("sessions", "s1", &doc("a", &["one"])).unwrap();
    assert_eq!(db.stats().docs, 1);
}

#[test]
fn a_new_schema_version_starts_empty() {
    let dir = TempDir::new("vec-version");
    let path = dir.path("vectors.db");
    let db = VectorDb::open(&path).unwrap();
    db.put_doc("sessions", "s1", &doc("a", &["one"])).unwrap();
    drop(db);
    assert_eq!(VectorDb::open(&path).unwrap().stats().docs, 1);
}

#[test]
fn a_damaged_file_with_an_old_log_opens_empty() {
    let dir = TempDir::new("vec-damaged-wal");
    let first = dir.path("a/vectors.db");
    let seed = VectorDb::open(&first).unwrap();
    seed.put_doc("sessions", "s1", &doc("old", &["stale words"]))
        .unwrap();
    drop(seed);
    let live = rusqlite::Connection::open(&first).unwrap();
    live.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    live.execute("INSERT INTO meta (k, v) VALUES ('x', 'y')", [])
        .unwrap();
    let wal = std::fs::read(dir.path("a/vectors.db-wal")).unwrap();
    assert!(!wal.is_empty());
    let mut main = std::fs::read(&first).unwrap();
    main[..16].copy_from_slice(b"not sqlite at a\0");
    let path = dir.path("b/vectors.db");
    std::fs::create_dir_all(dir.path("b")).unwrap();
    std::fs::write(&path, main).unwrap();
    std::fs::write(dir.path("b/vectors.db-wal"), &wal).unwrap();
    let db = VectorDb::open(&path).unwrap();
    assert_eq!(db.stats().docs, 0);
    db.put_doc("sessions", "s2", &doc("new", &["fresh words"]))
        .unwrap();
    assert_eq!(db.stats().docs, 1);
    assert!(dir.path("b/vectors.db.damaged").exists());
    drop(live);
}

#[test]
fn a_damaged_file_with_a_large_log_of_schema_pages_opens_empty() {
    let dir = TempDir::new("vec-damaged-schema-wal");
    let first = dir.path("a/vectors.db");
    let seed = VectorDb::open(&first).unwrap();
    seed.with(|c| c.pragma_update(None, "wal_autocheckpoint", 0).unwrap());
    for i in 0..4 {
        seed.put_doc(
            "sessions",
            &format!("s{i}"),
            &doc(&format!("old-{i}"), &["stale words", "more stale"]),
        )
        .unwrap();
    }
    let mut wal = std::fs::read(dir.path("a/vectors.db-wal")).unwrap();
    assert!(wal.len() > 4096 * 3, "wal len {}", wal.len());
    let mut main = std::fs::read(&first).unwrap();
    main.resize(main.len().max(4096), b'x');
    main[..16].copy_from_slice(b"not sqlite at a\0");
    wal[..4].copy_from_slice(b"junk");
    let path = dir.path("b/vectors.db");
    std::fs::create_dir_all(dir.path("b")).unwrap();
    std::fs::write(&path, main).unwrap();
    std::fs::write(dir.path("b/vectors.db-wal"), &wal).unwrap();
    let db = VectorDb::open(&path).unwrap();
    assert!(dir.path("b/vectors.db.damaged").exists());
    assert_eq!(db.stats().docs, 0);
    db.put_doc("sessions", "s9", &doc("new", &["fresh words"]))
        .unwrap();
    assert_eq!(db.stats().docs, 1);
    drop(db);
    assert_eq!(VectorDb::open(&path).unwrap().stats().docs, 1);
    drop(seed);
}

#[test]
fn a_header_gone_bad_under_a_valid_log_is_read_from_the_log_not_replaced() {
    let dir = TempDir::new("vec-valid-schema-wal");
    let first = dir.path("a/vectors.db");
    let seed = VectorDb::open(&first).unwrap();
    seed.with(|c| c.pragma_update(None, "wal_autocheckpoint", 0).unwrap());
    for i in 0..3 {
        seed.put_doc(
            "sessions",
            &format!("s{i}"),
            &doc(&format!("kept-{i}"), &["words"]),
        )
        .unwrap();
    }
    let wal = std::fs::read(dir.path("a/vectors.db-wal")).unwrap();
    let mut main = std::fs::read(&first).unwrap();
    main.resize(main.len().max(4096), b'x');
    main[..16].copy_from_slice(b"not sqlite at a\0");
    let path = dir.path("b/vectors.db");
    std::fs::create_dir_all(dir.path("b")).unwrap();
    std::fs::write(&path, main).unwrap();
    std::fs::write(dir.path("b/vectors.db-wal"), &wal).unwrap();
    let db = VectorDb::open(&path).unwrap();
    assert!(!dir.path("b/vectors.db.damaged").exists());
    assert_eq!(db.stats().docs, 3);
    drop(seed);
}

#[test]
fn live_returns_only_hashes_still_in_chunks() {
    let db = VectorDb::memory().unwrap();
    assert!(db.live(&[]).is_empty());
    db.put_doc("sessions", "s1", &doc("a", &["one", "two"]))
        .unwrap();
    db.put_doc("sessions", "s1", &doc("b", &["three"])).unwrap();
    let queried = [hash("one"), hash("three"), hash("missing")];
    let want: HashSet<[u8; 16]> = [hash("one"), hash("three")].into();
    assert_eq!(db.live(&queried), want);
    db.remove_docs("sessions", &["a".to_string()]).unwrap();
    let after: HashSet<[u8; 16]> = [hash("three")].into();
    assert_eq!(db.live(&queried), after);
}

#[test]
fn a_valid_log_left_beside_a_set_aside_file_is_discarded_not_replayed() {
    let dir = TempDir::new("vec-damaged-valid-wal");
    let first = dir.path("a/vectors.db");
    let seed = VectorDb::open(&first).unwrap();
    for i in 0..3 {
        seed.put_doc(
            "sessions",
            &format!("s{i}"),
            &doc(&format!("old-{i}"), &["stale words"]),
        )
        .unwrap();
    }
    drop(seed);
    let live = rusqlite::Connection::open(&first).unwrap();
    live.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    live.pragma_update(None, "user_version", 0).unwrap();
    let wal = std::fs::read(dir.path("a/vectors.db-wal")).unwrap();
    assert!(!wal.is_empty());
    let mut main = std::fs::read(&first).unwrap();
    assert!(main.len() > 4096 * 2);
    for byte in main[4096..].iter_mut() {
        *byte = b'x';
    }
    let path = dir.path("b/vectors.db");
    std::fs::create_dir_all(dir.path("b")).unwrap();
    std::fs::write(&path, main).unwrap();
    std::fs::write(dir.path("b/vectors.db-wal"), &wal).unwrap();
    let db = VectorDb::open(&path).unwrap();
    assert!(dir.path("b/vectors.db.damaged").exists());
    assert_eq!(db.stats().docs, 0);
    db.put_doc("sessions", "s9", &doc("new", &["fresh words"]))
        .unwrap();
    assert_eq!(db.stats().docs, 1);
    drop(db);
    assert_eq!(VectorDb::open(&path).unwrap().stats().docs, 1);
    drop(live);
}

#[test]
fn a_main_file_damaged_beneath_a_current_log_is_set_aside_on_open() {
    let dir = TempDir::new("vec-damaged-current-wal");
    let first = dir.path("a/vectors.db");
    let seed = VectorDb::open(&first).unwrap();
    for i in 0..3 {
        seed.put_doc(
            "sessions",
            &format!("s{i}"),
            &doc(&format!("old-{i}"), &["stale words"]),
        )
        .unwrap();
    }
    drop(seed);
    let live = VectorDb::open(&first).unwrap();
    live.with(|c| c.pragma_update(None, "wal_autocheckpoint", 0).unwrap());
    live.put_doc(
        "sessions",
        "s7",
        &doc("later", &["later words", "and more later words"]),
    )
    .unwrap();
    let wal = std::fs::read(dir.path("a/vectors.db-wal")).unwrap();
    assert!(!wal.is_empty());
    let mut main = std::fs::read(&first).unwrap();
    for byte in main[4096..].iter_mut() {
        *byte = b'x';
    }
    let path = dir.path("b/vectors.db");
    std::fs::create_dir_all(dir.path("b")).unwrap();
    std::fs::write(&path, main).unwrap();
    std::fs::write(dir.path("b/vectors.db-wal"), &wal).unwrap();
    let db = VectorDb::open(&path).unwrap();
    assert!(dir.path("b/vectors.db.damaged").exists());
    assert_eq!(db.stats().docs, 0);
    db.put_doc("sessions", "s9", &doc("new", &["fresh words"]))
        .unwrap();
    assert_eq!(db.stats().docs, 1);
    drop(live);
}
