use super::guard::{allowed_folder, clean_path, folder_allowed, stays_inside};
use crate::sessions::testutil::TempDir;
use std::path::PathBuf;

#[test]
fn a_path_must_be_relative_and_stay_below() {
    assert_eq!(clean_path("src/a.rs"), Ok("src/a.rs".to_owned()));
    assert_eq!(clean_path("./a.rs"), Ok("./a.rs".to_owned()));
    for bad in ["", "/etc/passwd", "../x", "a/../../x", "a/..", "a\0b"] {
        assert!(clean_path(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_folder_is_allowed_when_it_is_a_known_one_or_below_it() {
    let t = TempDir::new("guard-known");
    t.write("proj/src/a.rs", "x");
    t.write("other/b.rs", "x");
    let known = vec![t.path("proj")];
    assert!(folder_allowed(&t.path("proj"), &known));
    assert!(folder_allowed(&t.path("proj/src"), &known));
    assert!(!folder_allowed(&t.path("other"), &known));
    assert!(!folder_allowed(&t.path("proj/missing"), &known));
    assert!(!folder_allowed(&t.path("proj/../other"), &known));
    assert!(!folder_allowed(std::path::Path::new("proj"), &known));
    assert!(!folder_allowed(&t.path("proj"), &[]));
    assert!(!folder_allowed(
        &t.path("proj"),
        &[PathBuf::from("/definitely/not/here")]
    ));
}

#[cfg(unix)]
#[test]
fn a_link_out_of_a_known_folder_is_not_below_it() {
    let t = TempDir::new("guard-link-cwd");
    t.write("proj/a.rs", "x");
    t.write("secret/s.txt", "x");
    std::os::unix::fs::symlink(t.path("secret"), t.path("proj/out")).unwrap();
    assert!(!folder_allowed(&t.path("proj/out"), &[t.path("proj")]));
}

#[test]
fn a_plain_path_stays_inside_even_when_it_does_not_exist() {
    let t = TempDir::new("guard-inside");
    t.write("a/b.rs", "x");
    assert!(stays_inside(t.root(), "a/b.rs"));
    assert!(stays_inside(t.root(), "a/gone.rs"));
    assert!(stays_inside(t.root(), "gone/dir/file.rs"));
}

#[cfg(unix)]
#[test]
fn links_that_leave_the_repository_are_refused_and_inner_ones_are_not() {
    let t = TempDir::new("guard-links");
    t.write("repo/in/real.txt", "x");
    t.write("outside/secret.txt", "x");
    let repo = t.path("repo");
    std::os::unix::fs::symlink(t.path("outside"), repo.join("dir")).unwrap();
    std::os::unix::fs::symlink(t.path("outside/secret.txt"), repo.join("file")).unwrap();
    std::os::unix::fs::symlink(repo.join("in"), repo.join("inner")).unwrap();
    std::os::unix::fs::symlink("/nonexistent/nowhere", repo.join("dangling")).unwrap();
    assert!(!stays_inside(&repo, "dir/secret.txt"));
    assert!(!stays_inside(&repo, "dir"));
    assert!(!stays_inside(&repo, "file"));
    assert!(!stays_inside(&repo, "dangling"));
    assert!(stays_inside(&repo, "inner/real.txt"));
    assert!(stays_inside(&repo, "in/real.txt"));
}

#[cfg(unix)]
#[test]
fn an_allowed_folder_comes_back_as_its_canonical_path() {
    let t = TempDir::new("guard-canonical");
    t.write("proj/src/a.rs", "x");
    std::os::unix::fs::symlink(t.path("proj"), t.path("alias")).unwrap();
    let real = std::fs::canonicalize(t.path("proj/src")).unwrap();
    let got = allowed_folder(&t.path("alias/src"), &[t.path("proj")]);
    assert_eq!(got, Some(real));
    assert_eq!(allowed_folder(&t.path("alias/src"), &[]), None);
}
