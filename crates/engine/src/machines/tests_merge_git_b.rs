use super::fakes_jobs::lock;
use super::tests_merge_git::{entry, sh, Fx, GIT};

fn push_from_another_clone(fx: &Fx, marker: &str) -> String {
    let root = &fx.root;
    let origin = fx.origin();
    format!(
        "[ -e '{root}/{marker}' ] || {{ touch '{root}/{marker}'; rm -rf '{root}/other'; {GIT} clone -q '{origin}' '{root}/other' 2>/dev/null; cd '{root}/other'; \
         {GIT} -c user.email=t@t -c user.name=t commit -q --allow-empty -m 'someone else'; {GIT} push -q origin main; }}; true"
    )
}

#[test]
fn push_rejected_because_origin_moved_then_undo_and_merge_again() {
    let fx = Fx::new();
    let commit = fx.branch("sd/issue-7");
    let e = entry(7, "sd/issue-7", &commit);
    let check = push_from_another_clone(&fx, "pushed-once");
    let before = fx.origin_main();

    let first = fx.merge_with(&e, &check);
    assert!(
        first
            .error
            .as_deref()
            .is_some_and(|e| e.starts_with("push failed")),
        "{:?}",
        first.error
    );
    assert!(first.merged && first.checked && !first.pushed);
    assert!(first.can_retry_push());
    assert!(first.can_undo());
    assert_ne!(fx.origin_main(), before, "someone else's push is on origin");
    let theirs = fx.origin_main();

    let again = fx.retry(&e);
    assert!(!again.pushed);
    assert_eq!(fx.origin_main(), theirs);

    let undone = fx.undo(&e);
    assert_eq!(undone.error, None);
    assert_eq!(fx.mac_main(), before);
    assert_eq!(fx.git(&fx.mac(), "status --porcelain"), "");

    let second = fx.merge_with(&e, &check);
    assert_eq!(second.error, None);
    assert!(second.pushed && second.closed);
    assert_eq!(fx.git(&fx.origin(), "rev-parse refs/heads/main^1"), theirs);
    assert_eq!(fx.git(&fx.origin(), "rev-parse refs/heads/main^2"), commit);
}

#[test]
fn main_ahead_by_this_items_merge_is_reported_and_can_be_undone() {
    let fx = Fx::new();
    let commit = fx.branch("sd/issue-7");
    let e = entry(7, "sd/issue-7", &commit);
    let before = fx.origin_main();
    let (mac, machine) = (fx.mac(), fx.machine_path());
    fx.git(
        &mac,
        &format!("fetch -q --no-tags '{machine}' '+refs/heads/sd/issue-7:refs/machines/m1/7'"),
    );
    fx.git(
        &mac,
        "merge -q --no-ff -m 'Merge sd/issue-7 (acme/app#7)' refs/machines/m1/7",
    );
    let local = fx.mac_main();

    let o = fx.merge(&e);
    assert!(
        o.error
            .as_deref()
            .is_some_and(|e| e.contains("merged locally, not pushed")),
        "{:?}",
        o.error
    );
    assert!(o.merged);
    assert!(!o.checked);
    assert!(!o.pushed);
    assert_eq!(o.commit.as_deref(), Some(local.as_str()));
    assert!(!o.can_retry_push(), "the check never ran on this merge");
    assert!(o.can_undo());
    fx.assert_nothing_reached_origin(&before);
    assert_eq!(fx.mac_main(), local);

    let undone = fx.undo(&e);
    assert_eq!(undone.error, None);
    assert_eq!(fx.mac_main(), before);

    fx.runner.clear();
    let second = fx.merge(&e);
    assert_eq!(second.error, None);
    assert!(second.pushed);
}

#[test]
fn unpushed_commits_of_the_users_own_are_not_merged_onto_or_pushed() {
    let fx = Fx::new();
    let commit = fx.branch("sd/issue-7");
    let e = entry(7, "sd/issue-7", &commit);
    let before = fx.origin_main();
    fx.write(&fx.mac(), "mine.txt", "mine\n");
    fx.git(&fx.mac(), "add -A");
    fx.git(&fx.mac(), "commit -q -m 'my own work'");
    let mine = fx.mac_main();

    let o = fx.merge(&e);
    assert_eq!(
        o.error.as_deref(),
        Some("main has 1 commit(s) that are not on origin; push or undo them yourself")
    );
    assert!(!o.merged);
    assert!(!o.can_undo());
    fx.assert_nothing_reached_origin(&before);
    assert_eq!(fx.mac_main(), mine);

    let refused = fx.undo(&e);
    assert!(
        refused.error.is_some(),
        "undo must not touch commits that are not a merge of this branch"
    );
    assert_eq!(fx.mac_main(), mine);
}

fn local_merge(fx: &Fx, branch: &str, issue: i64) -> String {
    let (mac, machine) = (fx.mac(), fx.machine_path());
    fx.git(
        &mac,
        &format!("fetch -q --no-tags '{machine}' '+refs/heads/{branch}:refs/machines/m1/{issue}'"),
    );
    fx.git(
        &mac,
        &format!("merge -q --no-ff -m 'Merge {branch}' refs/machines/m1/{issue}"),
    );
    fx.mac_main()
}

#[test]
fn undo_refuses_when_the_merge_is_not_this_items_or_there_is_more_on_top() {
    let fx = Fx::new();
    let seven = fx.branch("sd/issue-7");
    let eight = fx.worker_branch("sd/issue-8", "g.txt", "g\n", None);
    let e7 = entry(7, "sd/issue-7", &seven);

    let merge_of_eight = local_merge(&fx, "sd/issue-8", 8);
    assert!(fx.undo(&e7).error.is_some());
    assert_eq!(fx.mac_main(), merge_of_eight);

    let e8 = entry(8, "sd/issue-8", &eight);
    fx.write(&fx.mac(), "mine.txt", "mine\n");
    fx.git(&fx.mac(), "add -A");
    fx.git(&fx.mac(), "commit -q -m 'my own work'");
    let top = fx.mac_main();
    assert!(fx.undo(&e8).error.is_some());
    assert_eq!(fx.mac_main(), top);
}

#[test]
fn undo_refuses_when_other_unpushed_commits_sit_under_the_merge() {
    let fx = Fx::new();
    let seven = fx.branch("sd/issue-7");
    fx.write(&fx.mac(), "mine.txt", "mine\n");
    fx.git(&fx.mac(), "add -A");
    fx.git(&fx.mac(), "commit -q -m 'my own work'");
    let merged = local_merge(&fx, "sd/issue-7", 7);
    let o = fx.undo(&entry(7, "sd/issue-7", &seven));
    assert!(
        o.error.is_some(),
        "two commits ahead of origin: the user's work would be lost"
    );
    assert_eq!(fx.mac_main(), merged);
}

#[test]
fn undo_refuses_a_three_parent_head() {
    let fx = Fx::new();
    let seven = fx.branch("sd/issue-7");
    let _eight = fx.worker_branch("sd/issue-8", "g.txt", "g\n", None);
    let (mac, machine) = (fx.mac(), fx.machine_path());
    fx.git(
        &mac,
        &format!(
            "fetch -q --no-tags '{machine}' '+refs/heads/sd/issue-7:refs/machines/m1/7' '+refs/heads/sd/issue-8:refs/machines/m1/8'"
        ),
    );
    fx.git(
        &mac,
        "merge -q --no-ff -m octopus refs/machines/m1/7 refs/machines/m1/8",
    );
    assert_eq!(
        fx.git(&mac, "rev-list --parents -n 1 HEAD")
            .split(' ')
            .count(),
        4,
        "a 3-parent merge"
    );
    let octopus = fx.mac_main();
    let o = fx.undo(&entry(7, "sd/issue-7", &seven));
    assert!(o.error.is_some());
    assert_eq!(fx.mac_main(), octopus);
}

#[test]
fn undo_goes_back_to_the_first_parent_and_keeps_origin_untouched() {
    let fx = Fx::new();
    let seven = fx.branch("sd/issue-7");
    let before = fx.origin_main();
    local_merge(&fx, "sd/issue-7", 7);
    let o = fx.undo(&entry(7, "sd/issue-7", &seven));
    assert_eq!(o.error, None);
    assert!(!o.merged);
    assert_eq!(fx.mac_main(), before);
    assert_eq!(fx.git(&fx.mac(), "status --porcelain"), "");
    fx.assert_nothing_reached_origin(&before);
}

#[test]
fn merge_uses_the_verified_commit_even_if_the_ref_is_rewritten_meanwhile() {
    let fx = Fx::new();
    let commit = fx.branch("sd/issue-7");
    let mac = fx.mac();
    let other = fx.git(&mac, "commit-tree 'HEAD^{tree}' -p HEAD -m other");
    let root = fx.root.clone();
    *lock(&fx.runner.after_call) = Some(Box::new(move |argv: &[String]| {
        if argv.contains(&"rev-parse".to_string())
            && argv.last().map(String::as_str) == Some("refs/machines/m1/7")
        {
            sh(
                &root,
                &format!("{GIT} -C '{mac}' update-ref refs/machines/m1/7 {other}"),
            );
        }
    }));
    let o = fx.merge(&entry(7, "sd/issue-7", &commit));
    assert_eq!(o.error, None);
    assert_eq!(
        fx.git(&fx.origin(), "rev-parse refs/heads/main^2"),
        commit,
        "the other commit was merged"
    );
}

#[test]
fn a_merge_that_never_starts_reports_its_own_error() {
    let fx = Fx::new();
    let m = fx.machine_path();
    fx.git(&m, "checkout -q --orphan unrelated");
    fx.git(&m, "rm -rf -q .");
    fx.write(&m, "z.txt", "z\n");
    fx.git(&m, "add -A");
    fx.git(&m, "commit -q -m unrelated");
    let commit = fx.git(&m, "rev-parse HEAD");
    fx.git(&m, "checkout -q main");
    let before = fx.origin_main();
    let o = fx.merge(&entry(7, "unrelated", &commit));
    assert!(
        o.error
            .as_deref()
            .is_some_and(|e| e.contains("unrelated histories")),
        "{:?}",
        o.error
    );
    assert!(!o.error.as_deref().unwrap_or("").contains("aborted"));
    assert!(o.conflict_files.is_empty());
    assert!(!o.merged);
    fx.assert_nothing_reached_origin(&before);
    assert_eq!(fx.git(&fx.mac(), "status --porcelain"), "");
}

#[test]
fn a_real_conflict_still_aborts_and_names_the_files() {
    let fx = Fx::new();
    let commit = fx.worker_branch("sd/issue-7", "f.txt", "base\nfrom the machine\n", None);
    fx.write(&fx.mac(), "f.txt", "base\nfrom the mac\n");
    fx.git(&fx.mac(), "commit -q -am 'mac edit'");
    fx.git(&fx.mac(), "push -q origin main");
    let before = fx.origin_main();
    let o = fx.merge(&entry(7, "sd/issue-7", &commit));
    assert_eq!(o.conflict_files, vec!["f.txt"]);
    assert!(o
        .error
        .as_deref()
        .is_some_and(|e| e.contains("merge conflict in f.txt")));
    assert_eq!(
        fx.git(&fx.mac(), "status --porcelain"),
        "",
        "the conflict was aborted"
    );
    assert_eq!(fx.mac_main(), before);
    let gh: Vec<Vec<String>> = fx
        .runner
        .gh_calls()
        .into_iter()
        .map(|c| c[..2].to_vec())
        .collect();
    assert_eq!(gh, vec![vec!["issue", "comment"], vec!["issue", "edit"]]);
}

#[test]
fn branch_names_git_would_read_as_options_or_cannot_be_refs_are_refused() {
    let fx = Fx::new();
    let commit = fx.branch("sd/issue-7");
    let before = fx.origin_main();
    for bad in [
        "-x",
        "--upload-pack=touch /tmp/pwned",
        "a..b",
        "x y",
        "a~1",
        "",
        "@{-1}",
        "feature/",
    ] {
        fx.runner.clear();
        let o = fx.merge(&entry(7, bad, &commit));
        assert!(o.error.is_some(), "'{bad}' was accepted");
        assert!(!o.fetched, "'{bad}' reached the fetch");
        assert!(!o.merged);
        assert!(
            !fx.runner
                .calls()
                .iter()
                .any(|c| c.iter().any(|a| a.starts_with("ssh://"))),
            "'{bad}' reached the machine"
        );
    }
    assert_eq!(fx.origin_main(), before);
    assert_eq!(fx.mac_main(), before);
}

#[test]
fn failed_check_whose_reset_fails_is_a_hard_error_with_no_retry() {
    let fx = Fx::new();
    let commit = fx.branch("sd/issue-7");
    let before = fx.origin_main();
    let check =
        format!("touch \"$({GIT} rev-parse --git-dir)/index.lock\"; echo tests failed; exit 1");
    let o = fx.merge_with(&entry(7, "sd/issue-7", &commit), &check);
    fx.sh(&format!("rm -f '{}/.git/index.lock'", fx.mac()));
    let want = format!(
        "check failed and main could not be put back; fix it by hand in {}",
        fx.mac()
    );
    assert!(
        o.error.as_deref().is_some_and(|e| e.starts_with(&want)),
        "{:?}",
        o.error
    );
    assert!(o.merged, "the merge is still on main");
    assert!(!o.checked);
    assert!(
        !o.can_retry_push(),
        "Retry push would push a merge whose check failed"
    );
    assert!(!o.pushed);
    fx.assert_nothing_reached_origin(&before);
}

#[test]
fn failed_check_puts_main_back() {
    let fx = Fx::new();
    let commit = fx.branch("sd/issue-7");
    let before = fx.origin_main();
    let o = fx.merge_with(
        &entry(7, "sd/issue-7", &commit),
        "echo tests failed; exit 1",
    );
    assert!(o
        .error
        .as_deref()
        .is_some_and(|e| e.contains("check failed") && e.contains("tests failed")));
    assert!(!o.merged);
    assert!(!o.can_retry_push());
    assert!(!o.can_undo());
    assert_eq!(fx.mac_main(), before);
    assert_eq!(fx.git(&fx.mac(), "status --porcelain"), "");
    fx.assert_nothing_reached_origin(&before);
}

#[test]
fn edit_during_the_check_blocks_the_push_and_keeps_the_merge_local() {
    let fx = Fx::new();
    let commit = fx.branch("sd/issue-7");
    let before = fx.origin_main();
    let e = entry(7, "sd/issue-7", &commit);
    let o = fx.merge_with(&e, "printf 'user edit\\n' >> f.txt; true");
    assert_eq!(
        o.error.as_deref(),
        Some("main has local changes made during the check; nothing pushed")
    );
    assert!(o.merged);
    assert!(!o.pushed);
    assert!(
        !o.can_retry_push(),
        "the check ran on a tree that is not the commit to push"
    );
    assert!(o.can_undo());
    fx.assert_nothing_reached_origin(&before);
    assert_eq!(fx.git(&fx.mac(), "status --porcelain"), "M f.txt");

    let refused = fx.undo(&e);
    assert!(
        refused
            .error
            .as_deref()
            .is_some_and(|e| e.contains("local changes")),
        "{:?}",
        refused.error
    );
    assert_eq!(fx.git(&fx.mac(), "rev-parse HEAD^2"), commit);

    fx.git(&fx.mac(), "checkout -q -- f.txt");
    let done = fx.undo(&e);
    assert_eq!(done.error, None);
    assert_eq!(fx.mac_main(), before);
    assert_eq!(fx.git(&fx.mac(), "status --porcelain"), "");
}
