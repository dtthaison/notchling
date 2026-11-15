use super::blob::sha256_of;
use super::members::Term;
use super::snapshot::{dir_digest, Manifest, Part, PartKind};
use super::standby_store::StandbyStore;
use super::testkit::id;
use crate::sessions::testutil::TempDir;
use std::fs;
use std::path::{Path, PathBuf};

fn primary() -> super::NodeId {
    id("primary")
}

fn store(tmp: &TempDir) -> StandbyStore {
    StandbyStore::new(&tmp.path("cluster"), &primary())
}

fn manifest(set: &str, seq: u64, parts: Vec<Part>) -> Manifest {
    Manifest {
        set: set.to_string(),
        seq,
        term: Term {
            n: 1,
            node: primary(),
        },
        home: primary(),
        created_ms: 1_000 + seq as i64,
        sessions: Vec::new(),
        parts,
    }
}

fn part(kind: PartKind, path: &str, bytes: &[u8]) -> Part {
    Part {
        kind,
        path: PathBuf::from(path),
        sha256: sha256_of(bytes),
        len: bytes.len() as u64,
    }
}

fn source(tmp: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = tmp.path(&format!("src/{name}"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path
}

fn stored(tmp: &TempDir, set: &str, original: &str) -> PathBuf {
    tmp.path(&format!(
        "cluster/standby/{}/sets/{set}/parts/{}",
        primary(),
        original.trim_start_matches('/')
    ))
}

fn tail_file(tmp: &TempDir, original: &str) -> PathBuf {
    tmp.path(&format!(
        "cluster/standby/{}/tails/parts/{}",
        primary(),
        original.trim_start_matches('/')
    ))
}

fn dir_source(tmp: &TempDir, name: &str, files: &[(&str, &[u8])]) -> (PathBuf, Part) {
    let root = tmp.path(&format!("src/{name}"));
    let mut entries = Vec::new();
    let mut total = 0u64;
    for (rel, bytes) in files {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        entries.push((rel.to_string(), bytes.len() as u64, sha256_of(bytes)));
        total += bytes.len() as u64;
    }
    fs::create_dir_all(&root).unwrap();
    let dir = Part {
        kind: PartKind::Dir,
        path: PathBuf::from("/prim/pages"),
        sha256: dir_digest(&entries),
        len: total,
    };
    (root, dir)
}

fn send(store: &StandbyStore, tmp: &TempDir, m: &Manifest, bodies: &[(&Part, &[u8])]) {
    store.begin(m).unwrap();
    for (n, (p, bytes)) in bodies.iter().enumerate() {
        let from = source(tmp, &format!("{}-{n}", m.set), bytes);
        store.part(&m.set, p, &from).unwrap();
    }
}

fn tail_set(set: &str, seq: u64, from: u64, bytes: &[u8]) -> (Manifest, Part) {
    let tail = part(PartKind::Tail { from }, "/prim/t.jsonl", bytes);
    (manifest(set, seq, vec![tail.clone()]), tail)
}

#[test]
fn complete_set_becomes_current() {
    let tmp = TempDir::new("standby-complete");
    let store = store(&tmp);
    let db = part(PartKind::Db, "/prim/cluster.db", b"db-bytes");
    let file = part(PartKind::File, "/prim/devices.json", b"{}");
    let tail = part(PartKind::Tail { from: 0 }, "/prim/t.jsonl", b"one\ntwo\n");
    let (dir_root, dir) = dir_source(&tmp, "pages", &[("a.html", b"<a>"), ("img/b.png", b"png")]);
    let m = manifest(
        "s-1",
        1,
        vec![db.clone(), file.clone(), tail.clone(), dir.clone()],
    );
    store.begin(&m).unwrap();
    assert!(store.current().is_none());
    for (p, bytes) in [
        (&db, &b"db-bytes"[..]),
        (&file, &b"{}"[..]),
        (&tail, &b"one\ntwo\n"[..]),
    ] {
        let from = source(&tmp, "x", bytes);
        store.part("s-1", p, &from).unwrap();
    }
    store.part("s-1", &dir, &dir_root).unwrap();
    assert!(store.current().is_none());
    store.commit("s-1").unwrap();
    assert_eq!(store.current(), Some(m));
    assert_eq!(
        fs::read(stored(&tmp, "s-1", "/prim/cluster.db")).unwrap(),
        b"db-bytes"
    );
    assert_eq!(
        fs::read(stored(&tmp, "s-1", "/prim/pages/img/b.png")).unwrap(),
        b"png"
    );
    assert_eq!(
        fs::read(tail_file(&tmp, "/prim/t.jsonl")).unwrap(),
        b"one\ntwo\n"
    );
    assert_eq!(
        store.lengths().get(Path::new("/prim/t.jsonl")).copied(),
        Some(8)
    );
    assert!(!tmp
        .path(&format!("cluster/standby/{}/sets/s-1.partial", primary()))
        .exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&stored(&tmp, "s-1", "/prim/cluster.db")), 0o600);
        assert_eq!(mode(&tail_file(&tmp, "/prim/t.jsonl")), 0o600);
        assert_eq!(
            mode(&tmp.path(&format!("cluster/standby/{}", primary()))),
            0o700
        );
        assert_eq!(mode(stored(&tmp, "s-1", "/prim/pages").as_path()), 0o700);
        assert_eq!(
            mode(&tmp.path(&format!("cluster/standby/{}/current.json", primary()))),
            0o600
        );
    }
}

#[test]
fn partial_set_never_becomes_current() {
    let tmp = TempDir::new("standby-partial");
    let first = part(PartKind::File, "/prim/a", b"one");
    let m1 = manifest("s-1", 1, vec![first.clone()]);
    let store1 = store(&tmp);
    send(&store1, &tmp, &m1, &[(&first, b"one")]);
    store1.commit("s-1").unwrap();

    let (m2, tail) = tail_set("s-2", 2, 0, b"transcript");
    let file2 = part(PartKind::File, "/prim/b", b"two");
    let mut m2 = m2;
    m2.parts.push(file2.clone());
    send(&store1, &tmp, &m2, &[(&tail, b"transcript")]);
    assert!(store1.commit("s-2").is_err());
    assert_eq!(store1.current(), Some(m1.clone()));
    assert!(store1.lengths().is_empty());
    assert!(!tail_file(&tmp, "/prim/t.jsonl").exists());

    let reopened = StandbyStore::new(&tmp.path("cluster"), &primary());
    assert_eq!(reopened.current(), Some(m1));
    assert_eq!(reopened.missing(&m2), vec![file2]);
}

#[test]
fn bad_hash_part_is_refused() {
    let tmp = TempDir::new("standby-hash");
    let store = store(&tmp);
    let file = part(PartKind::File, "/prim/a", b"right");
    let other = part(PartKind::File, "/prim/b", b"other");
    let m = manifest("s-1", 1, vec![file.clone()]);
    store.begin(&m).unwrap();
    let wrong = source(&tmp, "wrong", b"wrong");
    assert!(store.part("s-1", &file, &wrong).is_err());
    let short = source(&tmp, "short", b"righ");
    assert!(store.part("s-1", &file, &short).is_err());
    let right = source(&tmp, "right", b"right");
    assert!(store.part("s-1", &other, &right).is_err());
    assert!(store.commit("s-1").is_err());
    assert_eq!(store.missing(&m), vec![file.clone()]);

    store.part("s-1", &file, &right).unwrap();
    fs::write(stored_partial(&tmp, "s-1", "/prim/a"), b"tampered").unwrap();
    assert!(store.commit("s-1").is_err());
    assert!(store.current().is_none());

    let (dir_root, dir) = dir_source(&tmp, "pages", &[("a", b"1")]);
    let mut wrong_dir = dir.clone();
    wrong_dir.sha256 = sha256_of(b"not the digest");
    let m2 = manifest("s-2", 2, vec![wrong_dir.clone()]);
    store.begin(&m2).unwrap();
    assert!(store.part("s-2", &wrong_dir, &dir_root).is_err());
}

fn stored_partial(tmp: &TempDir, set: &str, original: &str) -> PathBuf {
    tmp.path(&format!(
        "cluster/standby/{}/sets/{set}.partial/parts/{}",
        primary(),
        original.trim_start_matches('/')
    ))
}

#[test]
fn tail_part_appends_only_at_its_offset() {
    let tmp = TempDir::new("standby-tail");
    let store = store(&tmp);
    let (m1, t1) = tail_set("s-1", 1, 0, b"hello ");
    send(&store, &tmp, &m1, &[(&t1, b"hello ")]);
    store.commit("s-1").unwrap();

    let (m2, t2) = tail_set("s-2", 2, 3, b"world");
    store.begin(&m2).unwrap();
    let from = source(&tmp, "w", b"world");
    let refusal = store.part("s-2", &t2, &from).unwrap_err();
    assert!(refusal.contains("is at 6, not 3"), "{refusal}");
    assert_eq!(
        fs::read(tail_file(&tmp, "/prim/t.jsonl")).unwrap(),
        b"hello "
    );

    let (m3, t3) = tail_set("s-3", 3, 6, b"world");
    store.begin(&m3).unwrap();
    store.part("s-3", &t3, &from).unwrap();
    assert_eq!(
        fs::read(tail_file(&tmp, "/prim/t.jsonl")).unwrap(),
        b"hello "
    );
    store.commit("s-3").unwrap();
    assert_eq!(
        fs::read(tail_file(&tmp, "/prim/t.jsonl")).unwrap(),
        b"hello world"
    );
    assert_eq!(
        store.lengths().get(Path::new("/prim/t.jsonl")).copied(),
        Some(11)
    );

    let (m4, t4) = tail_set("s-4", 4, 6, b"world");
    store.begin(&m4).unwrap();
    store.part("s-4", &t4, &from).unwrap();
    store.commit("s-4").unwrap();
    assert_eq!(
        fs::read(tail_file(&tmp, "/prim/t.jsonl")).unwrap(),
        b"hello world"
    );

    let (m5, t5) = tail_set("s-5", 5, 0, b"fresh");
    send(&store, &tmp, &m5, &[(&t5, b"fresh")]);
    store.commit("s-5").unwrap();
    assert_eq!(
        fs::read(tail_file(&tmp, "/prim/t.jsonl")).unwrap(),
        b"fresh"
    );
}

#[test]
fn a_cut_append_is_finished_by_the_next_commit() {
    let tmp = TempDir::new("standby-cut");
    let store = store(&tmp);
    let (m1, t1) = tail_set("s-1", 1, 0, b"abc");
    send(&store, &tmp, &m1, &[(&t1, b"abc")]);
    store.commit("s-1").unwrap();
    let (m2, t2) = tail_set("s-2", 2, 3, b"defgh");
    send(&store, &tmp, &m2, &[(&t2, b"defgh")]);
    fs::write(tail_file(&tmp, "/prim/t.jsonl"), b"abcde").unwrap();
    store.commit("s-2").unwrap();
    assert_eq!(
        fs::read(tail_file(&tmp, "/prim/t.jsonl")).unwrap(),
        b"abcdefgh"
    );
}

#[test]
fn resume_sends_only_missing_parts() {
    let tmp = TempDir::new("standby-resume");
    let a = part(PartKind::File, "/prim/a", b"aaa");
    let b = part(PartKind::Db, "/prim/b.db", b"bbb");
    let c = part(PartKind::Tail { from: 0 }, "/prim/c.jsonl", b"ccc");
    let m = manifest("s-1", 1, vec![a.clone(), b.clone(), c.clone()]);
    let first = store(&tmp);
    assert!(first.missing(&m).len() == 3);
    send(&first, &tmp, &m, &[(&b, b"bbb")]);
    drop(first);

    let again = StandbyStore::new(&tmp.path("cluster"), &primary());
    again.begin(&m).unwrap();
    assert_eq!(again.missing(&m), vec![a.clone(), c.clone()]);
    let from = source(&tmp, "a", b"aaa");
    again.part("s-1", &a, &from).unwrap();
    assert_eq!(again.missing(&m), vec![c.clone()]);
    assert!(again.commit("s-1").is_err());
    let from = source(&tmp, "c", b"ccc");
    again.part("s-1", &c, &from).unwrap();
    assert!(again.missing(&m).is_empty());
    again.commit("s-1").unwrap();
    assert!(again.missing(&m).is_empty());
    assert_eq!(again.current(), Some(m.clone()));

    let fresh = manifest("s-2", 2, vec![a.clone()]);
    again.begin(&fresh).unwrap();
    let d = part(PartKind::File, "/prim/d", b"ddd");
    let other = manifest("s-3", 3, vec![b.clone(), d.clone()]);
    again.begin(&other).unwrap();
    assert_eq!(again.missing(&other), vec![d]);
    assert!(!tmp
        .path(&format!("cluster/standby/{}/sets/s-2.partial", primary()))
        .exists());
}

#[test]
fn unchanged_parts_come_from_the_current_set() {
    let tmp = TempDir::new("standby-reuse");
    let store = store(&tmp);
    let same = part(PartKind::File, "/prim/devices.json", b"{}");
    let db = part(PartKind::Db, "/prim/cluster.db", b"v1");
    let m1 = manifest("s-1", 1, vec![same.clone(), db]);
    store.begin(&m1).unwrap();
    store
        .part("s-1", &same, &source(&tmp, "same", b"{}"))
        .unwrap();
    store
        .part(
            "s-1",
            &part(PartKind::Db, "/prim/cluster.db", b"v1"),
            &source(&tmp, "v1", b"v1"),
        )
        .unwrap();
    store.commit("s-1").unwrap();

    let db2 = part(PartKind::Db, "/prim/cluster.db", b"v2");
    let m2 = manifest("s-2", 2, vec![same.clone(), db2.clone()]);
    store.begin(&m2).unwrap();
    assert_eq!(store.missing(&m2), vec![db2.clone()]);
    store.part("s-2", &db2, &source(&tmp, "v2", b"v2")).unwrap();
    store.commit("s-2").unwrap();
    assert_eq!(
        fs::read(stored(&tmp, "s-2", "/prim/devices.json")).unwrap(),
        b"{}"
    );
    assert_eq!(
        fs::read(stored(&tmp, "s-1", "/prim/cluster.db")).unwrap(),
        b"v1"
    );
}

#[test]
fn prune_keeps_two() {
    let tmp = TempDir::new("standby-prune");
    let store = store(&tmp);
    for seq in 1..=4u64 {
        let body = format!("v{seq}");
        let p = part(PartKind::File, "/prim/a", body.as_bytes());
        let m = manifest(&format!("s-{seq}"), seq, vec![p.clone()]);
        send(&store, &tmp, &m, &[(&p, body.as_bytes())]);
        store.commit(&m.set).unwrap();
    }
    store.prune(2);
    let sets = tmp.path(&format!("cluster/standby/{}/sets", primary()));
    let mut left: Vec<String> = fs::read_dir(sets)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, vec!["s-3".to_string(), "s-4".to_string()]);
    assert_eq!(store.current().map(|m| m.set), Some("s-4".to_string()));
    store.prune(1);
    assert!(stored(&tmp, "s-4", "/prim/a").exists());
    assert!(!stored(&tmp, "s-3", "/prim/a").exists());
}

#[test]
fn paths_that_escape_the_store_are_refused() {
    let tmp = TempDir::new("standby-escape");
    let store = store(&tmp);
    for bad in [
        "relative/x",
        "/a/../../b",
        "/a/./b",
        "/",
        "",
        "/ok/../../../etc/passwd",
    ] {
        let p = part(PartKind::File, bad, b"x");
        assert!(
            store.begin(&manifest("s-1", 1, vec![p])).is_err(),
            "{bad:?}"
        );
    }
    for set in ["../s-1", "s/1", "", ".", "s-1.partial", "a b"] {
        assert!(
            store.begin(&manifest(set, 1, Vec::new())).is_err(),
            "{set:?}"
        );
    }
    let one = part(PartKind::File, "/a/b", b"x");
    let twice = manifest("s-1", 1, vec![one.clone(), one.clone()]);
    assert!(store.begin(&twice).is_err());
    let nested = manifest(
        "s-1",
        1,
        vec![
            part(PartKind::Dir, "/a", b""),
            part(PartKind::File, "/a/b", b"x"),
        ],
    );
    assert!(store.begin(&nested).is_err());
    let mut foreign = manifest("s-1", 1, Vec::new());
    foreign.home = id("someone-else");
    assert!(store.begin(&foreign).is_err());
    assert!(!tmp.path("etc").exists());
}

#[cfg(unix)]
#[test]
fn links_in_the_store_are_never_followed() {
    let tmp = TempDir::new("standby-links");
    let target = tmp.path("elsewhere");
    fs::create_dir_all(&target).unwrap();
    let home = tmp.path(&format!("cluster/standby/{}", primary()));
    fs::create_dir_all(&home).unwrap();
    std::os::unix::fs::symlink(&target, home.join("sets")).unwrap();
    let store = store(&tmp);
    let p = part(PartKind::File, "/prim/a", b"x");
    assert!(store.begin(&manifest("s-1", 1, vec![p.clone()])).is_err());
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);

    fs::remove_file(home.join("sets")).unwrap();
    let store = StandbyStore::new(&tmp.path("cluster"), &primary());
    let m = manifest("s-1", 1, vec![p.clone()]);
    store.begin(&m).unwrap();
    let partial = home.join("sets/s-1.partial/parts/prim");
    fs::create_dir_all(partial.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &partial).unwrap();
    let from = source(&tmp, "x", b"x");
    assert!(store.part("s-1", &p, &from).is_err());
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);

    let link_source = tmp.path("src/link");
    std::os::unix::fs::symlink(&from, &link_source).unwrap();
    fs::remove_file(&partial).unwrap();
    assert!(store.part("s-1", &p, &link_source).is_err());

    let (dir_root, dir) = dir_source(&tmp, "pages", &[("a", b"1")]);
    std::os::unix::fs::symlink(&from, dir_root.join("evil")).unwrap();
    let m2 = manifest("s-2", 2, vec![dir.clone()]);
    store.begin(&m2).unwrap();
    assert!(store.part("s-2", &dir, &dir_root).is_err());
}

#[test]
fn an_older_set_is_refused_and_a_held_one_is_a_no_op() {
    let tmp = TempDir::new("standby-older");
    let store = store(&tmp);
    let p = part(PartKind::File, "/prim/a", b"x");
    let m5 = manifest("s-5", 5, vec![p.clone()]);
    send(&store, &tmp, &m5, &[(&p, b"x")]);
    store.commit("s-5").unwrap();
    let older = manifest("s-3", 3, vec![p.clone()]);
    assert!(store.begin(&older).unwrap_err().contains("older"));
    store.begin(&m5).unwrap();
    assert!(store.missing(&m5).is_empty());
    assert_eq!(store.current(), Some(m5));
}

#[test]
fn ack_book_keeps_each_node_and_survives_a_restart() {
    use super::ack_book::AckBook;
    let tmp = TempDir::new("ack-book");
    let book = AckBook::new(&tmp.path("cluster"));
    let one = id("one");
    let two = id("two");
    let lengths = |n: u64| [(PathBuf::from("/a/t.jsonl"), n)].into_iter().collect();
    book.record(&one, "s-1", lengths(5)).unwrap();
    book.record(&two, "s-1", lengths(9)).unwrap();
    book.record(&one, "s-2", lengths(7)).unwrap();
    assert_eq!(book.get(&one).unwrap().set, "s-2");
    assert_eq!(book.get(&two).unwrap().lengths, lengths(9));
    let reopened = AckBook::new(&tmp.path("cluster"));
    assert_eq!(reopened.all(), book.all());
    let relative = [(PathBuf::from("relative"), 1)].into_iter().collect();
    assert!(book.record(&one, "s-3", relative).is_err());
    assert_eq!(book.get(&one).unwrap().set, "s-2");
    book.forget(&two);
    assert!(AckBook::new(&tmp.path("cluster")).get(&two).is_none());
}
