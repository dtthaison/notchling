use crate::git_args::{fresh_args, ref_arg, worktree_arg, FRESH_READS};
use crate::limits_ops_tests::TempDir;
use notchling_engine::git::refs::RefRow;
use notchling_engine::git::worktrees::WorktreeEntry;
use notchling_local_api::ErrorCode;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn entry(path: &Path) -> WorktreeEntry {
    WorktreeEntry {
        path: path.to_string_lossy().into_owned(),
        head: Some("a".repeat(40)),
        branch: Some("main".into()),
        ..WorktreeEntry::default()
    }
}

fn row(refname: &str) -> RefRow {
    RefRow {
        refname: refname.into(),
        commit: "a".repeat(40),
        ..RefRow::default()
    }
}

fn folder(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.path(name);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn forbidden<T: std::fmt::Debug>(result: Result<T, notchling_local_api::ApiError>) {
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Forbidden);
}

#[test]
fn listed_worktree_accepted_as_canonical() {
    let dir = TempDir::new("args-ok");
    let main = folder(&dir, "main");
    let lane = folder(&dir, "lane");
    let listed = vec![entry(&main), entry(&lane)];
    let got = worktree_arg(lane.to_str().unwrap(), &listed).unwrap();
    assert_eq!(got, std::fs::canonicalize(&lane).unwrap());
    let link = dir.path("alias");
    std::os::unix::fs::symlink(&lane, &link).unwrap();
    let got = worktree_arg(link.to_str().unwrap(), &listed).unwrap();
    assert_eq!(got, std::fs::canonicalize(&lane).unwrap());
}

#[test]
fn worktree_not_listed_refused() {
    let dir = TempDir::new("args-unlisted");
    let main = folder(&dir, "main");
    let other = folder(&dir, "other");
    let listed = vec![entry(&main)];
    forbidden(worktree_arg(other.to_str().unwrap(), &listed));
    forbidden(worktree_arg(dir.path("gone").to_str().unwrap(), &listed));
    forbidden(worktree_arg("", &listed));
    forbidden(worktree_arg(main.to_str().unwrap(), &[]));
}

#[test]
fn relative_worktree_refused() {
    let dir = TempDir::new("args-relative");
    let main = folder(&dir, "main");
    let listed = vec![entry(&main)];
    forbidden(worktree_arg("main", &listed));
    forbidden(worktree_arg("./main", &listed));
}

#[test]
fn worktree_with_dotdot_refused() {
    let dir = TempDir::new("args-dotdot");
    let main = folder(&dir, "main");
    let lane = folder(&dir, "lane");
    let listed = vec![entry(&lane)];
    let sneaky = format!("{}/../lane", main.display());
    assert_eq!(
        std::fs::canonicalize(&sneaky).unwrap(),
        std::fs::canonicalize(&lane).unwrap()
    );
    forbidden(worktree_arg(&sneaky, &listed));
}

#[test]
fn worktree_link_to_elsewhere_refused() {
    let dir = TempDir::new("args-link");
    let main = folder(&dir, "main");
    let elsewhere = folder(&dir, "elsewhere");
    let link = dir.path("lane");
    std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
    let listed = vec![entry(&main), entry(&link)];
    let got = worktree_arg(link.to_str().unwrap(), &listed).unwrap();
    assert_eq!(got, std::fs::canonicalize(&elsewhere).unwrap());
    let listed = vec![entry(&main)];
    forbidden(worktree_arg(link.to_str().unwrap(), &listed));
    let into = dir.path("into-main");
    std::os::unix::fs::symlink(&main, &into).unwrap();
    let listed = vec![entry(&elsewhere)];
    forbidden(worktree_arg(into.to_str().unwrap(), &listed));
}

#[test]
fn prunable_or_bare_worktree_refused() {
    let dir = TempDir::new("args-flags");
    let bare = folder(&dir, "bare");
    let gone = folder(&dir, "gone");
    let mut bare_entry = entry(&bare);
    bare_entry.bare = true;
    let mut gone_entry = entry(&gone);
    gone_entry.prunable = true;
    let listed = vec![bare_entry, gone_entry];
    forbidden(worktree_arg(bare.to_str().unwrap(), &listed));
    forbidden(worktree_arg(gone.to_str().unwrap(), &listed));
}

#[test]
fn ref_outside_heads_and_remotes_refused() {
    let listed = vec![
        row("refs/heads/main"),
        row("refs/tags/v1"),
        row("HEAD"),
        row("refs/stash"),
    ];
    forbidden(ref_arg("refs/tags/v1", &listed));
    forbidden(ref_arg("HEAD", &listed));
    forbidden(ref_arg("refs/stash", &listed));
    forbidden(ref_arg("main", &listed));
    assert_eq!(
        ref_arg("refs/heads/main", &listed).unwrap(),
        "refs/heads/main"
    );
}

#[test]
fn ref_starting_with_dash_refused() {
    let listed = vec![row("-c"), row("--output=x"), row("refs/heads/main")];
    forbidden(ref_arg("-c", &listed));
    forbidden(ref_arg("--output=x", &listed));
    forbidden(ref_arg("", &listed));
    forbidden(ref_arg("refs/heads/../x", &[row("refs/heads/../x")]));
    forbidden(ref_arg("refs/heads/a b", &[row("refs/heads/a b")]));
}

#[test]
fn ref_not_listed_refused() {
    let listed = vec![row("refs/heads/main"), row("refs/remotes/origin/main")];
    forbidden(ref_arg("refs/heads/other", &listed));
    forbidden(ref_arg("refs/remotes/origin/other", &listed));
    forbidden(ref_arg("refs/heads/main", &[]));
    assert_eq!(
        ref_arg("refs/remotes/origin/main", &listed).unwrap(),
        "refs/remotes/origin/main"
    );
}

#[test]
fn fresh_unknown_ignored() {
    let dir = TempDir::new("args-fresh");
    let main = folder(&dir, "main");
    let other = folder(&dir, "other");
    let listed = vec![entry(&main)];
    let raw = json!([
        other.to_str().unwrap(),
        7,
        null,
        "relative",
        format!("{}/../main", main.display()),
        main.to_str().unwrap(),
        main.to_str().unwrap()
    ]);
    let got = fresh_args(Some(&raw), &listed);
    assert_eq!(got, vec![std::fs::canonicalize(&main).unwrap()]);
    assert!(fresh_args(None, &listed).is_empty());
    assert!(fresh_args(Some(&json!("x")), &listed).is_empty());
    assert!(fresh_args(Some(&Value::Null), &listed).is_empty());
    assert!(fresh_args(Some(&json!({"a": 1})), &listed).is_empty());
}

#[test]
fn fresh_capped_at_two() {
    assert_eq!(FRESH_READS, 2);
    let dir = TempDir::new("args-cap");
    let paths: Vec<PathBuf> = ["a", "b", "c", "d"]
        .iter()
        .map(|name| folder(&dir, name))
        .collect();
    let listed: Vec<WorktreeEntry> = paths.iter().map(|p| entry(p)).collect();
    let raw = Value::Array(
        paths
            .iter()
            .map(|p| Value::String(p.to_string_lossy().into_owned()))
            .collect(),
    );
    let got = fresh_args(Some(&raw), &listed);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0], std::fs::canonicalize(&paths[0]).unwrap());
    assert_eq!(got[1], std::fs::canonicalize(&paths[1]).unwrap());
}
