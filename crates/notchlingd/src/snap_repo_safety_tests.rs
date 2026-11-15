use crate::snap_repo::read_repo;
use crate::snap_repo_tests::{add_submodule, bump_mtime, launcher, plain_git, unsandboxed, Rig};
use notchling_engine::cluster::snapshot::RepoFacts;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

type Entries = BTreeMap<PathBuf, (u64, SystemTime)>;

fn walk(dir: &Path, base: &Path, into: &mut Entries) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let meta = std::fs::symlink_metadata(&path).unwrap();
        let rel = path.strip_prefix(base).unwrap().to_path_buf();
        into.insert(rel, (meta.len(), meta.modified().unwrap()));
        if meta.is_dir() {
            walk(&path, base, into);
        }
    }
}

struct Picture {
    entries: Entries,
    index: Vec<u8>,
    index_mtime: SystemTime,
    refs: BTreeMap<PathBuf, Vec<u8>>,
    packed: Option<Vec<u8>>,
    stash: String,
}

fn picture(rig: &Rig) -> Picture {
    let dot = rig.work.join(".git");
    let mut entries = Entries::new();
    entries.insert(PathBuf::new(), {
        let meta = std::fs::metadata(&dot).unwrap();
        (meta.len(), meta.modified().unwrap())
    });
    walk(&dot, &dot, &mut entries);
    let mut refs = BTreeMap::new();
    for rel in entries.keys().filter(|p| p.starts_with("refs")) {
        if dot.join(rel).is_file() {
            refs.insert(rel.clone(), std::fs::read(dot.join(rel)).unwrap());
        }
    }
    Picture {
        entries,
        index: std::fs::read(dot.join("index")).unwrap(),
        index_mtime: std::fs::metadata(dot.join("index"))
            .unwrap()
            .modified()
            .unwrap(),
        refs,
        packed: std::fs::read(dot.join("packed-refs")).ok(),
        stash: rig.w(&["stash", "list"]),
    }
}

#[test]
fn reading_state_never_changes_the_repo() {
    let rig = Rig::pushed();
    rig.t.write("work/c.txt", "one\ntwo\n");
    rig.t.write("work/loose.txt", "x\n");
    rig.w(&["stash", "push", "-q", "-u", "-m", "keep"]);
    rig.t.write("work/loose.txt", "x\n");
    rig.w(&["pack-refs", "--all"]);
    bump_mtime(&rig.work.join("a.txt"), 30);
    let before = picture(&rig);
    assert!(before.packed.is_some());
    assert!(before.stash.contains("keep"));
    let facts = rig.read().unwrap();
    assert!(facts.untracked);
    let after = picture(&rig);
    assert_eq!(before.index, after.index, "the index bytes changed");
    assert_eq!(
        before.index_mtime, after.index_mtime,
        "the index was touched"
    );
    assert_eq!(before.refs, after.refs);
    assert_eq!(before.packed, after.packed);
    assert_eq!(before.stash, after.stash);
    assert_eq!(
        before.entries, after.entries,
        "a file or folder under .git changed"
    );

    let control = plain_git(rig.t.root(), &rig.work, &["status", "--porcelain"]);
    assert!(control.status.success());
    let touched = picture(&rig);
    assert_ne!(
        before.index, touched.index,
        "a plain status leaves the index alone, so this test would prove nothing"
    );
}

fn trap(rig: &Rig, name: &str) -> String {
    let path = rig.t.path(name);
    let body = format!(
        "#!/bin/sh\ntouch '{}'\ncat \"$1\" 2>/dev/null\n",
        rig.marker.display()
    );
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.to_string_lossy().into_owned()
}

fn must_not_run(rig: &Rig, stale: &Path) -> RepoFacts {
    assert!(!rig.marker.exists());
    let out = plain_git(rig.t.root(), &rig.work, &["status"]);
    assert!(out.status.success());
    assert!(
        rig.marker.exists(),
        "the trap never fires without the guards, so this test would prove nothing"
    );
    std::fs::remove_file(&rig.marker).unwrap();
    bump_mtime(stale, 90);
    let mut last = None;
    for launcher in [
        &unsandboxed(&rig.t) as &dyn notchling_engine::control::Launcher,
        &launcher(&rig.t),
    ] {
        last = read_repo(&rig.work, launcher).unwrap();
        assert!(!rig.marker.exists(), "a program of the repository ran");
    }
    last.unwrap()
}

fn make_stale_and_dirty(rig: &Rig) {
    rig.t.write("work/c.txt", "one\ntwo\n");
    bump_mtime(&rig.work.join("a.txt"), 30);
}

#[test]
fn hooks_and_fsmonitor_never_run() {
    let rig = Rig::pushed();
    rig.w(&["config", "core.fsmonitor", &trap(&rig, "monitor.sh")]);
    make_stale_and_dirty(&rig);
    assert!(must_not_run(&rig, &rig.work.join("a.txt")).dirty);

    let rig = Rig::pushed();
    let hook = rig.work.join(".git/hooks/post-index-change");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::copy(trap(&rig, "hook.sh"), &hook).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    make_stale_and_dirty(&rig);
    assert!(must_not_run(&rig, &rig.work.join("a.txt")).dirty);

    let rig = Rig::pushed();
    rig.t.write("work/.gitattributes", "* filter=x\n");
    rig.w(&["add", ".gitattributes"]);
    rig.w(&["commit", "-q", "-m", "attributes"]);
    rig.w(&["push", "-q"]);
    rig.w(&["config", "filter.x.clean", &trap(&rig, "clean.sh")]);
    make_stale_and_dirty(&rig);
    assert!(must_not_run(&rig, &rig.work.join("a.txt")).dirty);
}

#[test]
fn submodule_filter_never_runs() {
    let rig = Rig::pushed();
    add_submodule(&rig);
    rig.git(
        &rig.work.join("mod"),
        &["config", "filter.x.clean", &trap(&rig, "sub-clean.sh")],
    );
    let stale = rig.work.join("mod/s.txt");
    bump_mtime(&stale, 30);
    let facts = must_not_run(&rig, &stale);
    assert!(facts.submodules);
}
