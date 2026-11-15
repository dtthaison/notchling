use super::repo_state::{movable, parse, Movable};
use super::repo_state_tests::{facts, out, pushed, status, top, with_upstream, OID};
use crate::cluster::snapshot::GitOutput;
use std::path::Path;

fn index(records: &[&str]) -> GitOutput {
    let mut text = records.join("\0");
    text.push('\0');
    out(0, &text)
}

#[test]
fn a_gone_upstream_is_no_upstream() {
    let mut o = pushed();
    o[1] = status(&[
        format!("# branch.oid {OID}"),
        "# branch.head main".to_owned(),
        "# branch.upstream origin/main".to_owned(),
    ]);
    o[2] = out(128, "");
    let f = facts(&o);
    assert_eq!(f.upstream, None);
    assert_eq!(f.unpushed, 0);
    assert_eq!(
        movable(Some(&f), "/w/repo"),
        Movable::No("main has no pushed upstream".to_owned())
    );
}

#[test]
fn an_upstream_that_isnt_on_origin_is_not_pushed() {
    for upstream in ["main", "fork/main", "origin-fork/main", "heads/origin/main"] {
        let mut o = pushed();
        o[1] = status(&[
            format!("# branch.oid {OID}"),
            "# branch.head topic".to_owned(),
            format!("# branch.upstream {upstream}"),
            "# branch.ab +0 -0".to_owned(),
        ]);
        let f = facts(&o);
        assert_eq!(
            movable(Some(&f), "/w/repo"),
            Movable::No("topic has no pushed upstream".to_owned()),
            "{upstream}"
        );
    }
}

#[test]
fn an_upstream_on_origin_with_a_slashed_branch_is_pushed() {
    let mut o = pushed();
    o[1] = status(&[
        format!("# branch.oid {OID}"),
        "# branch.head feature/x".to_owned(),
        "# branch.upstream origin/feature/x".to_owned(),
        "# branch.ab +0 -0".to_owned(),
    ]);
    assert_eq!(movable(Some(&facts(&o)), "/w/repo"), Movable::Yes);
}

#[test]
fn a_gitlink_in_the_index_means_submodules() {
    let mut o = pushed();
    o[4] = index(&["H 100644 aaa 0\ta.txt", "H 160000 bbb 0\tmod"]);
    let f = facts(&o);
    assert!(f.submodules);
    assert!(!f.dirty);
    assert_eq!(
        movable(Some(&f), "/w/repo"),
        Movable::No("/w/repo has submodules".to_owned())
    );
}

#[test]
fn submodules_are_reported_after_untracked_files() {
    let mut o = pushed();
    o[1] = with_upstream(&["? loose.txt"]);
    o[4] = index(&["H 160000 bbb 0\tmod"]);
    assert_eq!(
        movable(Some(&facts(&o)), "/w/repo"),
        Movable::No("/w/repo has untracked files".to_owned())
    );
}

#[test]
fn a_path_that_looks_like_a_gitlink_is_not_one() {
    let mut o = pushed();
    o[4] = index(&["H 100644 aaa 0\t160000 H 160000 x"]);
    assert!(!facts(&o).submodules);
}

#[test]
fn hidden_edits_count_as_uncommitted() {
    for tag in ["h", "S", "s"] {
        let mut o = pushed();
        o[4] = index(&[&format!("{tag} 100644 aaa 0\ta.txt")]);
        let f = facts(&o);
        assert!(f.dirty, "{tag}");
        assert_eq!(
            movable(Some(&f), "/w/repo"),
            Movable::No("/w/repo has uncommitted changes".to_owned())
        );
    }
}

#[test]
fn ordinary_index_tags_are_not_dirty() {
    let mut o = pushed();
    o[4] = index(&["H 100644 aaa 0\ta.txt", "M 100644 bbb 1\tb.txt"]);
    assert!(!facts(&o).dirty);
}

#[test]
fn a_failed_listing_is_an_error() {
    let mut o = pushed();
    o[4] = out(128, "");
    assert!(parse(Path::new("/w/repo"), &o).is_err());
}

#[test]
fn only_not_a_repository_means_none() {
    let dubious = GitOutput {
        code: 128,
        stdout: String::new(),
        stderr: "fatal: detected dubious ownership in repository at '/w/repo'\nmore\n".to_owned(),
    };
    let mut o = pushed();
    o[0] = dubious;
    let err = parse(Path::new("/w/repo"), &o).unwrap_err();
    assert!(err.contains("dubious ownership"), "{err}");
    assert!(!err.contains("more"), "{err}");
    o[0] = top("/w/repo");
    assert!(parse(Path::new("/w/repo"), &o).unwrap().is_some());
}
