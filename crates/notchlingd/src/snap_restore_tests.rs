use crate::snap_restore_testkit::{World, S1};
use notchling_engine::cluster::restore::{PathState, RestoreStep};
use std::fs;

fn head(w: &World, dir: &std::path::Path) -> String {
    w.rig.git(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

#[test]
fn restore_clones_a_missing_repo_at_its_commit() {
    let w = World::new();
    w.rig.commit_in(&w.repo, "b.txt");
    w.rig.w(&["push", "-q"]);
    let want = head(&w, &w.repo);
    let status = w.rig.w(&["status", "--porcelain"]);
    let transcript = w.transcript(S1, b"one\ntwo\n");
    w.stage(vec![w.entry(S1, &w.repo, &[&transcript], &[])], &[], &[]);
    fs::remove_dir_all(&w.repo).unwrap();
    fs::remove_file(&transcript).unwrap();

    let report = w.run(&[S1], false, false).unwrap();

    assert_eq!(report.steps, 2);
    assert_eq!(head(&w, &w.repo), want);
    assert_eq!(w.rig.w(&["status", "--porcelain"]), status);
    assert_eq!(w.rig.w(&["branch", "--show-current"]).trim(), "main");
    assert_eq!(
        w.rig
            .w(&["rev-parse", "--abbrev-ref", "@{upstream}"])
            .trim(),
        "origin/main"
    );
    assert_eq!(fs::read_to_string(w.repo.join("b.txt")).unwrap(), "more\n");
    assert_eq!(fs::read(&transcript).unwrap(), b"one\ntwo\n");
}

#[test]
fn restore_fast_forwards_a_clean_clone() {
    let w = World::new();
    let first = head(&w, &w.repo);
    w.rig.commit_in(&w.repo, "b.txt");
    w.rig.w(&["push", "-q"]);
    let second = head(&w, &w.repo);
    let transcript = w.transcript(S1, b"one\n");
    w.stage(vec![w.entry(S1, &w.repo, &[&transcript], &[])], &[], &[]);
    fs::remove_dir_all(&w.repo).unwrap();
    let origin = w.rig.origin.to_string_lossy().into_owned();
    w.rig.git(&w.home, &["clone", "-q", &origin, "work"]);
    w.rig.w(&["reset", "-q", "--hard", &first]);
    assert_eq!(head(&w, &w.repo), first);
    assert!(matches!(w.state(&w.repo), PathState::Clean { .. }));

    w.run(&[S1], false, false).unwrap();

    assert_eq!(head(&w, &w.repo), second);
    assert_eq!(w.rig.w(&["status", "--porcelain"]), "");
    assert_eq!(w.rig.w(&["branch", "--show-current"]).trim(), "main");
}

#[test]
fn restore_refuses_a_folder_with_its_own_changes() {
    let w = World::new();
    let transcript = w.transcript(S1, b"one\n");
    w.stage(vec![w.entry(S1, &w.repo, &[&transcript], &[])], &[], &[]);
    fs::remove_file(&transcript).unwrap();
    fs::write(w.repo.join("a.txt"), "my own work\n").unwrap();
    let before = head(&w, &w.repo);

    let why = w.run(&[S1], false, false).unwrap_err();

    assert_eq!(
        why,
        format!("{} on m1 has its own changes", w.repo.display())
    );
    assert_eq!(
        fs::read_to_string(w.repo.join("a.txt")).unwrap(),
        "my own work\n"
    );
    assert_eq!(head(&w, &w.repo), before);
    assert!(!transcript.exists());
    assert!(w.run(&[S1], true, false).is_err());
}

#[test]
fn restore_refuses_a_clone_on_another_branch() {
    let w = World::new();
    let transcript = w.transcript(S1, b"one\n");
    w.stage(vec![w.entry(S1, &w.repo, &[&transcript], &[])], &[], &[]);
    w.rig.w(&["checkout", "-q", "-b", "other"]);

    let why = w.run(&[S1], false, false).unwrap_err();

    assert_eq!(why, format!("{} on m1 is on other", w.repo.display()));
    assert_eq!(w.rig.w(&["branch", "--show-current"]).trim(), "other");
}

#[test]
fn a_clone_with_commits_the_set_lacks_is_refused_and_left_alone() {
    let w = World::new();
    let first = head(&w, &w.repo);
    let transcript = w.transcript(S1, b"one\n");
    w.stage(vec![w.entry(S1, &w.repo, &[&transcript], &[])], &[], &[]);
    w.rig.commit_in(&w.repo, "mine.txt");
    w.rig.w(&["push", "-q"]);
    let mine = head(&w, &w.repo);
    assert_ne!(first, mine);

    let why = w.run(&[S1], false, false).unwrap_err();

    assert!(why.contains("commits that aren't in this set"), "{why}");
    assert_eq!(head(&w, &w.repo), mine);
}

#[test]
fn unmovable_session_is_not_restored() {
    let w = World::new();
    fs::write(w.repo.join("a.txt"), "dirty\n").unwrap();
    let transcript = w.transcript(S1, b"one\n");
    w.stage(vec![w.entry(S1, &w.repo, &[&transcript], &[])], &[], &[]);
    fs::remove_file(&transcript).unwrap();

    let why = w.run(&[S1], false, false).unwrap_err();

    assert!(why.starts_with(&format!("{S1} can't move: ")), "{why}");
    assert!(why.contains("uncommitted changes"), "{why}");
    assert!(!transcript.exists());
}

fn worktree(w: &World) -> std::path::PathBuf {
    let tree = w.at("wt");
    w.rig.w(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feat",
        &tree.to_string_lossy(),
    ]);
    w.rig.commit_in(&tree, "f.txt");
    w.rig.git(&tree, &["push", "-q", "-u", "origin", "feat"]);
    tree
}

#[test]
fn linked_worktree_is_added_to_its_main_repo() {
    let w = World::new();
    let tree = worktree(&w);
    let want = head(&w, &tree);
    let transcript = w.transcript(S1, b"one\n");
    let entry = w.entry(S1, &tree, &[&transcript], &[]);
    assert_eq!(
        entry.repo.as_ref().unwrap().main_repo.as_deref(),
        Some(w.repo.as_path())
    );
    w.stage(vec![entry], &[], &[]);
    w.rig
        .w(&["worktree", "remove", "--force", &tree.to_string_lossy()]);
    w.rig.w(&["branch", "-q", "-D", "feat"]);
    fs::remove_file(&transcript).unwrap();
    assert!(!tree.exists());

    w.run(&[S1], false, false).unwrap();

    assert_eq!(head(&w, &tree), want);
    assert_eq!(
        w.rig.git(&tree, &["branch", "--show-current"]).trim(),
        "feat"
    );
    assert_eq!(
        w.rig
            .git(&tree, &["rev-parse", "--abbrev-ref", "@{upstream}"])
            .trim(),
        "origin/feat"
    );
    assert!(w
        .rig
        .w(&["worktree", "list"])
        .contains(&tree.to_string_lossy().into_owned()));
    assert_eq!(fs::read(&transcript).unwrap(), b"one\n");
}

#[test]
fn a_linked_worktree_without_its_main_repository_is_refused_naming_it() {
    let w = World::new();
    let tree = worktree(&w);
    let transcript = w.transcript(S1, b"one\n");
    w.stage(vec![w.entry(S1, &tree, &[&transcript], &[])], &[], &[]);
    fs::remove_dir_all(&tree).unwrap();
    fs::remove_dir_all(&w.repo).unwrap();

    let why = w.run(&[S1], false, false).unwrap_err();

    assert!(why.starts_with(&w.repo.display().to_string()), "{why}");
    assert!(!tree.exists() && !w.repo.exists());
}

#[test]
fn the_plan_for_a_missing_clone_is_a_clone_then_the_transcript() {
    let w = World::new();
    let transcript = w.transcript(S1, b"one\n");
    w.stage(vec![w.entry(S1, &w.repo, &[&transcript], &[])], &[], &[]);
    fs::remove_dir_all(&w.repo).unwrap();
    fs::remove_file(&transcript).unwrap();
    let steps = w.plan(&[S1], false, false).unwrap();
    assert!(matches!(&steps[0], RestoreStep::CloneRepo { .. }));
    assert!(matches!(&steps[1], RestoreStep::PutFile { .. }));
}

#[test]
fn a_probe_tells_missing_empty_clean_changed_and_not_a_repository() {
    let w = World::new();
    assert!(matches!(w.state(&w.repo), PathState::Clean { branch, .. } if branch == "main"));
    assert_eq!(w.state(&w.at("nothing")), PathState::Missing);
    fs::create_dir_all(w.at("empty")).unwrap();
    assert_eq!(w.state(&w.at("empty")), PathState::Missing);
    fs::create_dir_all(w.at("plain")).unwrap();
    fs::write(w.at("plain/x"), "x").unwrap();
    assert_eq!(w.state(&w.at("plain")), PathState::Foreign);
    fs::write(w.repo.join("new.txt"), "untracked\n").unwrap();
    assert_eq!(w.state(&w.repo), PathState::Changed);
    fs::write(w.at("file"), "x").unwrap();
    assert_eq!(w.state(&w.at("file")), PathState::Foreign);
    std::os::unix::fs::symlink(&w.repo, w.at("link")).unwrap();
    assert_eq!(w.state(&w.at("link")), PathState::Foreign);
}

#[test]
fn a_symlinked_main_repository_is_refused_and_its_target_left_alone() {
    let w = World::new();
    let tree = worktree(&w);
    let transcript = w.transcript(S1, b"one\n");
    w.stage(vec![w.entry(S1, &tree, &[&transcript], &[])], &[], &[]);
    let elsewhere = crate::limits_ops_tests::TempDir::new("restore-main-out");
    let outside = elsewhere.path("main");
    let origin = w.rig.origin.to_string_lossy().into_owned();
    w.rig
        .git(elsewhere.root(), &["clone", "-q", &origin, "main"]);
    let before = w.rig.git(&outside, &["worktree", "list"]);
    fs::remove_dir_all(&tree).unwrap();
    w.rig.w(&["worktree", "prune"]);
    fs::remove_dir_all(&w.repo).unwrap();
    std::os::unix::fs::symlink(&outside, &w.repo).unwrap();
    fs::remove_file(&transcript).unwrap();

    let why = w.run(&[S1], false, false).unwrap_err();

    assert!(why.contains("isn't a repository"), "{why}");
    assert_eq!(w.rig.git(&outside, &["worktree", "list"]), before);
    assert!(!tree.exists() && !transcript.exists());
    assert!(crate::snap_restore_fs::real_folder(&w.home, &w.repo).is_err());
}

#[test]
fn an_empty_folder_at_a_worktrees_place_is_filled_or_refused_clearly() {
    let w = World::new();
    let tree = worktree(&w);
    let want = head(&w, &tree);
    let transcript = w.transcript(S1, b"one\n");
    w.stage(vec![w.entry(S1, &tree, &[&transcript], &[])], &[], &[]);
    w.rig
        .w(&["worktree", "remove", "--force", &tree.to_string_lossy()]);
    w.rig.w(&["branch", "-q", "-D", "feat"]);
    fs::create_dir_all(&tree).unwrap();

    match w.run(&[S1], false, false) {
        Ok(_) => assert_eq!(head(&w, &tree), want),
        Err(why) => assert!(
            why.contains(&tree.display().to_string()) && !why.contains("already exists in"),
            "{why}"
        ),
    }
}

#[test]
fn a_branch_that_already_exists_in_the_main_repository_is_named_as_that() {
    let w = World::new();
    let tree = worktree(&w);
    let transcript = w.transcript(S1, b"one\n");
    w.stage(vec![w.entry(S1, &tree, &[&transcript], &[])], &[], &[]);
    w.rig
        .w(&["worktree", "remove", "--force", &tree.to_string_lossy()]);

    let why = w.run(&[S1], false, false).unwrap_err();

    assert_eq!(why, "feat already exists in the main repository");
}

fn head_of(w: &World) -> String {
    w.rig.w(&["rev-parse", "HEAD"]).trim().to_owned()
}

fn origin_of(w: &World) -> String {
    w.rig.w(&["remote", "get-url", "origin"]).trim().to_owned()
}

#[test]
fn a_clone_never_lands_inside_another_repository() {
    let w = World::new();
    let inner = w.repo.join("inner");
    let step = RestoreStep::CloneRepo {
        origin: origin_of(&w),
        path: inner.clone(),
        branch: "main".to_owned(),
        head: head_of(&w),
    };

    let why = w.step(&step).unwrap_err();

    assert!(why.contains("is inside the repository"), "{why}");
    assert!(!inner.exists());
}

#[test]
fn a_repository_never_sits_behind_a_link() {
    let w = World::new();
    let real = w.at("real");
    fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, w.at("via")).unwrap();
    let step = RestoreStep::CloneRepo {
        origin: origin_of(&w),
        path: w.at("via/clone"),
        branch: "main".to_owned(),
        head: head_of(&w),
    };

    let why = w.step(&step).unwrap_err();

    assert!(why.contains("is a link"), "{why}");
    assert!(!real.join("clone").exists());
    assert_eq!(w.state(&w.at("via/clone")), PathState::Foreign);
    assert_eq!(w.state(&w.at("via")), PathState::Foreign);
}

#[test]
fn a_worktree_is_added_only_to_the_main_repository_the_set_names() {
    let w = World::new();
    let step = RestoreStep::AddWorktree {
        main: w.repo.clone(),
        origin: "git@example.invalid:o/other.git".to_owned(),
        path: w.at("wt2"),
        branch: "feat2".to_owned(),
        head: head_of(&w),
    };

    let why = w.step(&step).unwrap_err();

    assert!(why.contains("different remote"), "{why}");
    assert!(!w.at("wt2").exists());
    let not_main = RestoreStep::AddWorktree {
        main: w.at("nowhere"),
        origin: origin_of(&w),
        path: w.at("wt3"),
        branch: "feat3".to_owned(),
        head: head_of(&w),
    };
    assert!(w.step(&not_main).is_err());
    assert!(!w.at("wt3").exists());
}

#[test]
fn a_filter_the_folder_defines_never_runs_while_restoring() {
    let w = World::new();
    let clone = w.at("code/clone");
    fs::create_dir_all(w.at("code")).unwrap();
    w.rig.git(
        &w.at("code"),
        &["clone", "-q", &origin_of(&w), &clone.to_string_lossy()],
    );
    let marker = w.rig.marker.to_string_lossy().into_owned();
    w.rig.git(
        &clone,
        &[
            "config",
            "filter.evil.smudge",
            &format!("sh -c 'touch {marker}; cat'"),
        ],
    );
    fs::write(clone.join(".git/info/attributes"), "* filter=evil\n").unwrap();
    w.rig.commit_in(&w.repo, "new.txt");
    w.rig.w(&["push", "-q", "origin", "main"]);
    let step = RestoreStep::FastForward {
        path: clone.clone(),
        branch: "main".to_owned(),
        head: head_of(&w),
    };

    w.step(&step).unwrap();

    assert!(clone.join("new.txt").exists());
    assert!(!w.rig.marker.exists());
}
