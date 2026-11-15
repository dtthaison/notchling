use crate::snap_files::{copy_listed, stage_set};
use crate::snap_files_stage_tests::{facts, read_manifest, session, walk, Rig};
use crate::snap_files_tests::{age, hex, meta, plan, put, staged};
use notchling_engine::cluster::snapshot::{dir_digest, Movable, PartKind, MAX_FILE};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

fn set_names(stage: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(stage)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_symlinked_parent_still_hits_the_exclusions() {
    let rig = Rig::new("snap-safe-alias");
    let secret = rig.t.path("real/cluster/grants/a.json");
    put(&secret, b"secret");
    let alias = rig.t.path("alias");
    symlink(rig.t.path("real/cluster/grants"), &alias).unwrap();
    let mut p = plan("s-1");
    p.files = vec![alias.join("a.json")];
    p.tails = vec![(alias.join("a.json"), 0)];
    let manifest = stage_set(&p, &rig.stage, &meta()).unwrap();
    assert!(manifest.parts.is_empty());
}

#[test]
fn a_repo_behind_a_symlinked_parent_is_refused() {
    let rig = Rig::new("snap-safe-repo-alias");
    let root = rig.src("repo");
    put(&root.join("src/main.rs"), b"fn main() {}");
    let alias = rig.t.path("alias");
    symlink(root.join("src"), &alias).unwrap();
    let mut p = plan("s-2");
    p.sessions = vec![session(
        "a",
        &root.to_string_lossy(),
        Some(facts(&root)),
        Movable::Yes,
    )];
    p.files = vec![alias.join("main.rs")];
    assert!(stage_set(&p, &rig.stage, &meta()).is_err());
    assert!(!rig.stage.join("s-2").exists());
}

#[test]
fn a_dotdot_path_is_never_staged() {
    let rig = Rig::new("snap-safe-dotdot");
    let ok = rig.src("ok.json");
    put(&ok, b"ok");
    let mut p = plan("s-3");
    p.files = vec![rig.src("sub/../ok.json")];
    p.tails = vec![(rig.src("sub/../ok.json"), 0)];
    let manifest = stage_set(&p, &rig.stage, &meta()).unwrap();
    assert!(manifest.parts.is_empty());
}

#[test]
fn paths_in_or_around_the_stage_are_refused() {
    let rig = Rig::new("snap-safe-stage");
    let inside = rig.stage.join("other/x.json");
    put(&inside, b"x");
    let mut p = plan("s-4");
    p.files = vec![inside];
    assert!(stage_set(&p, &rig.stage, &meta()).is_err());
    let mut q = plan("s-5");
    q.files = vec![rig.t.path("data/cluster")];
    assert!(stage_set(&q, &rig.stage, &meta()).is_err());
}

#[test]
fn a_symlinked_stage_is_refused() {
    let rig = Rig::new("snap-safe-stage-link");
    let real = rig.t.path("elsewhere");
    fs::create_dir_all(&real).unwrap();
    fs::create_dir_all(rig.stage.parent().unwrap()).unwrap();
    symlink(&real, &rig.stage).unwrap();
    assert!(stage_set(&plan("s-6"), &rig.stage, &meta()).is_err());
    assert!(fs::read_dir(&real).unwrap().next().is_none());
}

#[test]
fn a_failed_restage_keeps_the_old_set() {
    let rig = Rig::new("snap-safe-restage");
    let a = rig.src("a.json");
    put(&a, b"one");
    let mut p = plan("s-7");
    p.files = vec![a.clone()];
    stage_set(&p, &rig.stage, &meta()).unwrap();
    age(&a, 10);
    assert!(stage_set(&p, &rig.stage, &meta()).is_err());
    assert_eq!(
        read_manifest(&rig.stage, "s-7").parts[0].sha256,
        hex(b"one")
    );
    assert_eq!(fs::read(staged(&rig.stage, "s-7", &a)).unwrap(), b"one");
    assert_eq!(set_names(&rig.stage), vec!["s-7".to_owned()]);
}

#[test]
fn no_staging_folder_is_left_after_a_success() {
    let rig = Rig::new("snap-safe-clean");
    let a = rig.src("a.json");
    put(&a, b"one");
    let mut p = plan("s-8");
    p.files = vec![a];
    stage_set(&p, &rig.stage, &meta()).unwrap();
    assert_eq!(set_names(&rig.stage), vec!["s-8".to_owned()]);
}

#[test]
fn a_set_id_that_names_a_staging_folder_is_refused() {
    let rig = Rig::new("snap-safe-id");
    assert!(stage_set(&plan("s-9.staging"), &rig.stage, &meta()).is_err());
}

#[test]
fn odd_names_in_a_dir_are_skipped() {
    let rig = Rig::new("snap-safe-names");
    let dir = rig.src("pages");
    put(&dir.join("good.html"), b"good");
    put(&dir.join("bad\nname.html"), b"bad");
    put(&dir.join("sub\nfolder/inner.html"), b"inner");
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::OsStr::from_bytes(b"latin\xff.html");
        let _ = fs::write(dir.join(name), b"bytes");
    }
    let mut p = plan("s-10");
    p.files = vec![dir.clone()];
    let manifest = stage_set(&p, &rig.stage, &meta()).unwrap();
    assert_eq!(manifest.parts.len(), 1);
    assert_eq!(manifest.parts[0].len, 4);
    assert_eq!(
        manifest.parts[0].sha256,
        dir_digest(&[("good.html".to_owned(), 4, hex(b"good"))])
    );
    let mut all: Vec<PathBuf> = Vec::new();
    walk(&rig.stage, &mut all);
    assert!(all
        .iter()
        .all(|path| !path.to_string_lossy().contains('\n')));
}

#[test]
fn a_file_that_vanishes_before_its_copy_is_skipped() {
    let rig = Rig::new("snap-safe-vanish");
    fs::create_dir_all(rig.t.path("out")).unwrap();
    let gone = rig.src("gone.html");
    let copied = copy_listed(&gone, &rig.t.path("out/gone.html"), &meta()).unwrap();
    assert_eq!(copied, None);
}

#[test]
fn a_tail_over_the_limit_is_skipped() {
    let rig = Rig::new("snap-safe-bigtail");
    let big = rig.src("proj/big.jsonl");
    fs::create_dir_all(big.parent().unwrap()).unwrap();
    fs::File::create(&big)
        .unwrap()
        .set_len(MAX_FILE + 10)
        .unwrap();
    age(&big, 3_600_000);
    let mut p = plan("s-11");
    p.tails = vec![(big.clone(), 0)];
    let manifest = stage_set(&p, &rig.stage, &meta()).unwrap();
    assert!(manifest.parts.is_empty());
    assert!(!staged(&rig.stage, "s-11", &big).exists());
}

#[test]
fn a_big_transcript_with_a_small_new_tail_is_staged() {
    let rig = Rig::new("snap-safe-latetail");
    let big = rig.src("proj/big.jsonl");
    fs::create_dir_all(big.parent().unwrap()).unwrap();
    fs::File::create(&big)
        .unwrap()
        .set_len(MAX_FILE + 1000)
        .unwrap();
    age(&big, 3_600_000);
    let mut p = plan("s-12");
    p.tails = vec![(big, MAX_FILE + 990)];
    let manifest = stage_set(&p, &rig.stage, &meta()).unwrap();
    assert_eq!(manifest.parts.len(), 1);
    assert_eq!(
        manifest.parts[0].kind,
        PartKind::Tail {
            from: MAX_FILE + 990
        }
    );
    assert_eq!(manifest.parts[0].len, 10);
}
