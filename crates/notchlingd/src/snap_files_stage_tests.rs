use crate::limits_ops_tests::TempDir;
use crate::snap_files::stage_set;
use crate::snap_files_tests::{age, hex, make_db, meta, node, plan, put, staged};
use notchling_engine::cluster::id::Tool;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::snapshot::{
    dir_digest, Manifest, Movable, PartKind, RepoFacts, SessionEntry, MAX_FILE,
};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

pub(crate) struct Rig {
    pub(crate) t: TempDir,
    pub(crate) stage: PathBuf,
}

impl Rig {
    pub(crate) fn new(tag: &str) -> Rig {
        let t = TempDir::new(tag);
        let stage = t.path("data/cluster/outgoing");
        Rig { t, stage }
    }

    pub(crate) fn src(&self, rel: &str) -> PathBuf {
        self.t.path(&format!("src/{rel}"))
    }
}

pub(crate) fn session(
    id: &str,
    cwd: &str,
    repo: Option<RepoFacts>,
    movable: Movable,
) -> SessionEntry {
    SessionEntry {
        id: id.to_owned(),
        tool: Tool::Claude,
        cwd: cwd.to_owned(),
        repo,
        movable,
        transcripts: Vec::new(),
        files: Vec::new(),
    }
}

pub(crate) fn facts(root: &Path) -> RepoFacts {
    RepoFacts {
        root: root.to_path_buf(),
        origin: Some("git@example.invalid:o/r.git".to_owned()),
        branch: Some("main".to_owned()),
        head: "0".repeat(40),
        upstream: Some("origin/main".to_owned()),
        unpushed: 0,
        dirty: false,
        untracked: false,
        main_repo: None,
        submodules: false,
    }
}

pub(crate) fn read_manifest(stage: &Path, set: &str) -> Manifest {
    let text = fs::read_to_string(stage.join(set).join("manifest.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn manifest_lists_every_part_with_hash() {
    let rig = Rig::new("snap-manifest");
    let db = rig.src("data/cluster.db");
    let _keep = make_db(&db, 40);
    let file = rig.src("data/devices.json");
    put(&file, b"[\"phone\"]");
    let dir = rig.src("pages");
    put(&dir.join("one/index.html"), b"<p>one</p>");
    put(&dir.join("two.html"), b"<p>two</p>");
    let tail = rig.src("proj/s.jsonl");
    put(&tail, b"line one\nline two\n");
    let mut p = plan("s-7");
    p.dbs = vec![db.clone()];
    p.files = vec![file.clone(), dir.clone()];
    p.tails = vec![(tail.clone(), 9)];
    let manifest = stage_set(&p, &rig.stage, &meta()).unwrap();
    assert_eq!(manifest.set, "s-7");
    assert_eq!(manifest.seq, 7);
    assert_eq!(manifest.term, Term { n: 3, node: node() });
    assert_eq!(manifest.home, node());
    assert_eq!(manifest.created_ms, 1_700_000_000_000);
    let kinds: Vec<&PartKind> = manifest.parts.iter().map(|p| &p.kind).collect();
    assert_eq!(
        kinds,
        vec![
            &PartKind::Db,
            &PartKind::File,
            &PartKind::Dir,
            &PartKind::Tail { from: 9 }
        ]
    );
    let paths: Vec<&PathBuf> = manifest.parts.iter().map(|p| &p.path).collect();
    assert_eq!(paths, vec![&db, &file, &dir, &tail]);
    let db_bytes = fs::read(staged(&rig.stage, "s-7", &db)).unwrap();
    assert_eq!(manifest.parts[0].sha256, hex(&db_bytes));
    assert_eq!(manifest.parts[0].len, db_bytes.len() as u64);
    assert_eq!(manifest.parts[1].sha256, hex(b"[\"phone\"]"));
    assert_eq!(manifest.parts[1].len, 9);
    let digest = dir_digest(&[
        ("one/index.html".to_owned(), 10, hex(b"<p>one</p>")),
        ("two.html".to_owned(), 10, hex(b"<p>two</p>")),
    ]);
    assert_eq!(manifest.parts[2].sha256, digest);
    assert_eq!(manifest.parts[2].len, 20);
    assert_eq!(
        fs::read(staged(&rig.stage, "s-7", &dir).join("one/index.html")).unwrap(),
        b"<p>one</p>"
    );
    assert_eq!(manifest.parts[3].sha256, hex(b"line two\n"));
    assert_eq!(manifest.parts[3].len, 9);
    assert_eq!(
        fs::read(staged(&rig.stage, "s-7", &tail)).unwrap(),
        b"line two\n"
    );
    assert_eq!(read_manifest(&rig.stage, "s-7"), manifest);
}

#[test]
fn manifest_records_each_session_and_whether_it_moves() {
    let rig = Rig::new("snap-sessions");
    let mut p = plan("s-1");
    p.sessions = vec![
        session(
            "a",
            "/work/a",
            Some(facts(Path::new("/work/a"))),
            Movable::Yes,
        ),
        session(
            "b",
            "/work/b",
            None,
            Movable::No("/work/b isn't a git repository".to_owned()),
        ),
    ];
    let manifest = stage_set(&p, &rig.stage, &meta()).unwrap();
    assert_eq!(manifest.sessions, p.sessions);
    assert_eq!(read_manifest(&rig.stage, "s-1").sessions, p.sessions);
}

pub(crate) fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        out.push(path.clone());
        if path.is_dir() {
            walk(&path, out);
        }
    }
}

#[test]
fn stage_is_0600_in_0700() {
    let rig = Rig::new("snap-modes");
    let db = rig.src("data/cluster.db");
    let _keep = make_db(&db, 10);
    let dir = rig.src("pages");
    put(&dir.join("a/b/c.html"), b"x");
    let tail = rig.src("proj/deep/s.jsonl");
    put(&tail, b"abc");
    let mut p = plan("s-2");
    p.dbs = vec![db];
    p.files = vec![dir];
    p.tails = vec![(tail, 0)];
    stage_set(&p, &rig.stage, &meta()).unwrap();
    let mut all = vec![rig.stage.clone()];
    walk(&rig.stage, &mut all);
    assert!(all.len() > 8);
    for path in all {
        let meta = fs::symlink_metadata(&path).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        let want = if meta.is_dir() { 0o700 } else { 0o600 };
        assert_eq!(mode, want, "{}", path.display());
    }
}

#[test]
fn excluded_paths_never_staged() {
    let rig = Rig::new("snap-excluded");
    let data = rig.t.path("data");
    let grants = data.join("cluster/grants/a.json");
    let logins = data.join("cluster/logins/b.json");
    let incoming = data.join("cluster/incoming/set/c.json");
    put(&grants, b"secret");
    put(&logins, b"secret");
    put(&incoming, b"secret");
    let big = rig.src("big.bin");
    fs::create_dir_all(big.parent().unwrap()).unwrap();
    fs::File::create(&big)
        .unwrap()
        .set_len(MAX_FILE + 1)
        .unwrap();
    let outside = rig.src("outside/real.txt");
    put(&outside, b"outside");
    let link = rig.src("link.txt");
    symlink(&outside, &link).unwrap();
    let dir = rig.src("pages");
    put(&dir.join("keep.html"), b"keep");
    symlink(&outside, dir.join("sneaky.html")).unwrap();
    put(&dir.join("cluster/grants/inner.json"), b"secret");
    let ok = rig.src("ok.json");
    put(&ok, b"ok");
    let mut p = plan("s-3");
    p.files = vec![
        grants.clone(),
        logins.clone(),
        incoming.clone(),
        data.join("cluster/grants"),
        big.clone(),
        link.clone(),
        dir.clone(),
        ok.clone(),
    ];
    let manifest = stage_set(&p, &rig.stage, &meta()).unwrap();
    let paths: Vec<&PathBuf> = manifest.parts.iter().map(|p| &p.path).collect();
    assert_eq!(paths, vec![&dir, &ok]);
    assert_eq!(manifest.parts[0].len, 4);
    let mut all = Vec::new();
    walk(&rig.stage, &mut all);
    for path in &all {
        let name = path.to_string_lossy();
        assert!(!name.contains("grants"), "{name}");
        assert!(!name.contains("logins"), "{name}");
        assert!(!name.contains("incoming"), "{name}");
        assert!(!name.contains("big.bin"), "{name}");
        assert!(!name.contains("link.txt"), "{name}");
        assert!(!name.contains("sneaky"), "{name}");
    }
    assert!(!staged(&rig.stage, "s-3", &dir)
        .join("keep.html")
        .is_symlink());
}

#[test]
fn busy_file_fails_the_whole_stage() {
    let rig = Rig::new("snap-busy-stage");
    let a = rig.src("a.json");
    let b = rig.src("b.json");
    put(&a, b"a");
    put(&b, b"b");
    age(&b, 100);
    let mut p = plan("s-4");
    p.files = vec![a, b.clone()];
    let err = stage_set(&p, &rig.stage, &meta()).unwrap_err();
    assert!(err.contains(&b.to_string_lossy().into_owned()), "{err}");
    assert!(!rig.stage.join("s-4").exists());
}

#[test]
fn busy_file_inside_a_dir_fails_the_whole_stage() {
    let rig = Rig::new("snap-busy-dir");
    let dir = rig.src("pages");
    put(&dir.join("a.html"), b"a");
    put(&dir.join("b.html"), b"b");
    age(&dir.join("b.html"), 10);
    let mut p = plan("s-5");
    p.files = vec![dir];
    let err = stage_set(&p, &rig.stage, &meta()).unwrap_err();
    assert!(err.contains("b.html"), "{err}");
    assert!(!rig.stage.join("s-5").exists());
}

#[test]
fn missing_file_is_skipped_but_a_missing_db_is_an_error() {
    let rig = Rig::new("snap-missing");
    let mut p = plan("s-6");
    p.files = vec![rig.src("nope.json")];
    p.tails = vec![(rig.src("nope.jsonl"), 4)];
    let manifest = stage_set(&p, &rig.stage, &meta()).unwrap();
    assert!(manifest.parts.is_empty());
    let mut q = plan("s-8");
    q.dbs = vec![rig.src("nope.db")];
    let err = stage_set(&q, &rig.stage, &meta()).unwrap_err();
    assert!(err.contains("nope.db"), "{err}");
    assert!(!rig.stage.join("s-8").exists());
}

#[test]
fn rewritten_tail_is_staged_whole() {
    let rig = Rig::new("snap-rewritten");
    let tail = rig.src("proj/s.jsonl");
    put(&tail, b"0123456789");
    let mut p = plan("s-9");
    p.tails = vec![(tail.clone(), 100)];
    let manifest = stage_set(&p, &rig.stage, &meta()).unwrap();
    assert_eq!(manifest.parts.len(), 1);
    assert_eq!(manifest.parts[0].kind, PartKind::Tail { from: 0 });
    assert_eq!(manifest.parts[0].len, 10);
    assert_eq!(manifest.parts[0].sha256, hex(b"0123456789"));
    assert_eq!(
        fs::read(staged(&rig.stage, "s-9", &tail)).unwrap(),
        b"0123456789"
    );
}

#[test]
fn a_path_inside_a_repo_is_refused() {
    let rig = Rig::new("snap-repo");
    let root = rig.src("repo");
    let code = root.join("src/main.rs");
    put(&code, b"fn main() {}");
    let ok = rig.src("ok.json");
    put(&ok, b"ok");
    let mut p = plan("s-10");
    p.sessions = vec![session(
        "a",
        &root.to_string_lossy(),
        Some(facts(&root)),
        Movable::Yes,
    )];
    p.files = vec![ok.clone(), code.clone()];
    let err = stage_set(&p, &rig.stage, &meta()).unwrap_err();
    assert!(err.contains(&code.to_string_lossy().into_owned()), "{err}");
    assert!(!rig.stage.join("s-10").exists());
    let mut q = p.clone();
    q.id = "s-11".to_owned();
    q.files = vec![ok];
    q.tails = vec![(code, 0)];
    assert!(stage_set(&q, &rig.stage, &meta()).is_err());
    let mut r = p;
    r.id = "s-12".to_owned();
    r.files = vec![rig.t.path("src")];
    assert!(stage_set(&r, &rig.stage, &meta()).is_err());
}

#[test]
fn a_set_id_cannot_leave_the_stage() {
    let rig = Rig::new("snap-id");
    for id in ["", "..", "a/b", "/abs"] {
        let p = plan(id);
        assert!(stage_set(&p, &rig.stage, &meta()).is_err(), "{id:?}");
    }
    assert!(!rig.t.path("data/cluster/manifest.json").exists());
}

#[test]
fn restaging_a_set_replaces_the_old_folder() {
    let rig = Rig::new("snap-restage");
    let a = rig.src("a.json");
    put(&a, b"one");
    let mut p = plan("s-13");
    p.files = vec![a.clone()];
    stage_set(&p, &rig.stage, &meta()).unwrap();
    put(&a, b"two!");
    let manifest = stage_set(&p, &rig.stage, &meta()).unwrap();
    assert_eq!(manifest.parts[0].len, 4);
    assert_eq!(fs::read(staged(&rig.stage, "s-13", &a)).unwrap(), b"two!");
}
