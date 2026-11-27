use super::*;
use crate::sessions::testutil::TempDir;

const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn branch_from_head() {
    let tmp = TempDir::new("ask-branch");
    tmp.write("repo/.git/HEAD", "ref: refs/heads/feature/ask-it\n");
    assert_eq!(
        branch(&tmp.path("repo")),
        Some("feature/ask-it".to_string())
    );
}

#[test]
fn branch_from_a_worktree_gitdir_file() {
    let tmp = TempDir::new("ask-worktree");
    tmp.write(
        "main/.git/worktrees/lane/HEAD",
        "ref: refs/heads/agent/ask\n",
    );
    let gitdir = tmp.path("main/.git/worktrees/lane");
    tmp.write(
        "lane/.git",
        &format!("gitdir: {}\n", gitdir.to_string_lossy()),
    );
    assert_eq!(branch(&tmp.path("lane")), Some("agent/ask".to_string()));
    tmp.write("rel/.git", "gitdir: ../main/.git/worktrees/lane\n");
    assert_eq!(branch(&tmp.path("rel")), Some("agent/ask".to_string()));
}

#[test]
fn detached_head_is_eight_chars() {
    let tmp = TempDir::new("ask-detached");
    tmp.write("repo/.git/HEAD", &format!("{HASH}\n"));
    assert_eq!(branch(&tmp.path("repo")), Some("01234567".to_string()));
}

#[test]
fn no_repository_is_none() {
    let tmp = TempDir::new("ask-none");
    std::fs::create_dir_all(tmp.path("plain/deeper")).unwrap();
    assert_eq!(branch(&tmp.path("plain/deeper")), None);
    assert_eq!(branch(&tmp.path("missing")), None);
}

#[test]
fn found_from_a_subfolder() {
    let tmp = TempDir::new("ask-sub");
    tmp.write("repo/.git/HEAD", "ref: refs/heads/main\n");
    std::fs::create_dir_all(tmp.path("repo/a/b/c")).unwrap();
    assert_eq!(branch(&tmp.path("repo/a/b/c")), Some("main".to_string()));
}

#[test]
fn unreadable_or_odd_heads_give_none() {
    let tmp = TempDir::new("ask-odd");
    tmp.write("a/.git/HEAD", "garbage\n");
    assert_eq!(branch(&tmp.path("a")), None);
    tmp.write("b/.git/HEAD", "ref: refs/remotes/origin/main\n");
    assert_eq!(branch(&tmp.path("b")), None);
    tmp.write("c/.git/HEAD", "ref: refs/heads/\n");
    assert_eq!(branch(&tmp.path("c")), None);
    tmp.write("d/.git", "not a gitdir line\n");
    assert_eq!(branch(&tmp.path("d")), None);
    std::fs::create_dir_all(tmp.path("e/.git")).unwrap();
    assert_eq!(branch(&tmp.path("e")), None);
}

#[test]
fn a_nearer_repository_wins() {
    let tmp = TempDir::new("ask-nested");
    tmp.write("outer/.git/HEAD", "ref: refs/heads/outer\n");
    tmp.write("outer/inner/.git/HEAD", "ref: refs/heads/inner\n");
    assert_eq!(branch(&tmp.path("outer/inner")), Some("inner".to_string()));
}
