use super::repo_state::{
    filter_probe, movable, parse, read_plan, without_filters, GitCall, GitOutput, Movable,
    RepoFacts,
};
use std::path::{Path, PathBuf};

pub(super) fn out(code: i32, stdout: &str) -> GitOutput {
    GitOutput {
        code,
        stdout: stdout.to_owned(),
        stderr: String::new(),
    }
}

pub(super) fn z(records: &[&str]) -> String {
    let mut text = records.join("\0");
    text.push('\0');
    text
}

pub(super) fn top(root: &str) -> GitOutput {
    out(0, &format!("{root}\n{root}/.git\n{root}/.git\n"))
}

pub(super) const OID: &str = "0123456789abcdef0123456789abcdef01234567";

fn head_records(extra: &[&str]) -> Vec<String> {
    let mut all = vec![
        format!("# branch.oid {OID}"),
        "# branch.head main".to_owned(),
    ];
    all.extend(extra.iter().map(|r| (*r).to_owned()));
    all
}

pub(super) fn status(records: &[String]) -> GitOutput {
    let refs: Vec<&str> = records.iter().map(String::as_str).collect();
    out(0, &z(&refs))
}

pub(super) fn with_upstream(entries: &[&str]) -> GitOutput {
    let mut extra = vec!["# branch.upstream origin/main", "# branch.ab +0 -0"];
    extra.extend(entries);
    status(&head_records(&extra))
}

pub(super) fn pushed() -> Vec<GitOutput> {
    vec![
        top("/w/repo"),
        with_upstream(&[]),
        out(0, "0\n"),
        out(0, "git@github.com:o/r.git\n"),
        out(0, "H 100644 aaa 0\ta.txt\0"),
    ]
}

pub(super) fn facts(outputs: &[GitOutput]) -> RepoFacts {
    parse(Path::new("/w/repo"), outputs).unwrap().unwrap()
}

#[test]
fn a_clean_pushed_tree_parses() {
    let f = facts(&pushed());
    assert_eq!(f.root, PathBuf::from("/w/repo"));
    assert_eq!(f.branch.as_deref(), Some("main"));
    assert_eq!(f.head, OID);
    assert_eq!(f.upstream.as_deref(), Some("origin/main"));
    assert_eq!(f.origin.as_deref(), Some("git@github.com:o/r.git"));
    assert_eq!((f.unpushed, f.dirty, f.untracked), (0, false, false));
    assert_eq!(f.main_repo, None);
    assert_eq!(movable(Some(&f), "/w/repo"), Movable::Yes);
}

#[test]
fn changed_entries_make_the_tree_dirty() {
    for record in [
        "1 .M N... 100644 100644 100644 aaa bbb a.txt",
        "1 .M S.M. 160000 160000 160000 aaa bbb sub",
        "u UU N... 100644 100644 100644 100644 a b c conflicted.txt",
    ] {
        let mut o = pushed();
        o[1] = with_upstream(&[record]);
        let f = facts(&o);
        assert!(f.dirty, "{record}");
        assert!(!f.untracked, "{record}");
    }
}

#[test]
fn a_rename_is_dirty_and_its_old_path_is_not_a_record() {
    let mut o = pushed();
    o[1] = with_upstream(&[
        "2 R. N... 100644 100644 100644 a b R100 new.txt",
        "? old.txt",
    ]);
    let f = facts(&o);
    assert!(f.dirty);
    assert!(!f.untracked);
}

#[test]
fn an_untracked_record_is_untracked_only() {
    let mut o = pushed();
    o[1] = with_upstream(&["? loose.txt"]);
    let f = facts(&o);
    assert!(f.untracked);
    assert!(!f.dirty);
}

#[test]
fn a_detached_head_has_no_branch() {
    let mut o = pushed();
    o[1] = status(&[
        format!("# branch.oid {OID}"),
        "# branch.head (detached)".to_owned(),
    ]);
    o[2] = out(128, "");
    let f = facts(&o);
    assert_eq!(f.branch, None);
    assert_eq!(f.head, OID);
    assert_eq!(f.upstream, None);
    assert_eq!(
        movable(Some(&f), "/w/repo"),
        Movable::No("/w/repo isn't on a branch".to_owned())
    );
}

#[test]
fn an_unborn_branch_has_an_empty_head() {
    let mut o = pushed();
    o[1] = status(&[
        "# branch.oid (initial)".to_owned(),
        "# branch.head main".to_owned(),
    ]);
    o[2] = out(128, "");
    o[3] = out(2, "");
    let f = facts(&o);
    assert_eq!(f.head, "");
    assert_eq!(f.branch.as_deref(), Some("main"));
    assert_eq!(f.origin, None);
}

#[test]
fn no_upstream_means_a_failed_count_is_fine() {
    let mut o = pushed();
    o[1] = status(&head_records(&[]));
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
fn a_failed_count_with_an_upstream_is_an_error() {
    let mut o = pushed();
    o[2] = out(128, "");
    assert!(parse(Path::new("/w/repo"), &o).is_err());
}

#[test]
fn an_ahead_count_is_read() {
    let mut o = pushed();
    o[2] = out(0, "3\n");
    let f = facts(&o);
    assert_eq!(f.unpushed, 3);
    assert_eq!(
        movable(Some(&f), "/w/repo"),
        Movable::No("main has 3 commits that aren't pushed".to_owned())
    );
}

#[test]
fn behind_the_upstream_is_still_movable() {
    let mut o = pushed();
    o[1] = status(&head_records(&[
        "# branch.upstream origin/main",
        "# branch.ab +0 -4",
    ]));
    let f = facts(&o);
    assert_eq!(f.unpushed, 0);
    assert_eq!(movable(Some(&f), "/w/repo"), Movable::Yes);
}

#[test]
fn a_garbled_count_is_an_error() {
    let mut o = pushed();
    o[2] = out(0, "many\n");
    assert!(parse(Path::new("/w/repo"), &o).is_err());
}

#[test]
fn no_origin_is_none() {
    let mut o = pushed();
    o[3] = out(2, "");
    let f = facts(&o);
    assert_eq!(f.origin, None);
    assert_eq!(
        movable(Some(&f), "/w/repo"),
        Movable::No("main has no pushed upstream".to_owned())
    );
}

#[test]
fn not_a_repository_is_none() {
    let o = vec![
        GitOutput {
            code: 128,
            stdout: String::new(),
            stderr: "fatal: not a git repository (or any of the parent directories): .git\n"
                .to_owned(),
        },
        out(128, ""),
        out(128, ""),
        out(128, ""),
        out(128, ""),
    ];
    assert_eq!(parse(Path::new("/w/none"), &o), Ok(None));
}

#[test]
fn another_rev_parse_failure_is_an_error() {
    let mut o = pushed();
    o[0] = out(1, "");
    assert!(parse(Path::new("/w/repo"), &o).is_err());
}

#[test]
fn a_failed_status_is_an_error() {
    let mut o = pushed();
    o[1] = out(128, "");
    assert!(parse(Path::new("/w/repo"), &o).is_err());
}

#[test]
fn the_wrong_number_of_outputs_is_an_error() {
    assert!(parse(Path::new("/w/repo"), &pushed()[..4]).is_err());
}

#[test]
fn a_linked_worktree_names_its_main_repository() {
    let mut o = pushed();
    o[0] = out(
        0,
        "/w/linked\n/w/main/.git/worktrees/linked\n/w/main/.git\n",
    );
    let f = facts(&o);
    assert_eq!(f.root, PathBuf::from("/w/linked"));
    assert_eq!(f.main_repo, Some(PathBuf::from("/w/main")));
}

#[test]
fn a_linked_worktree_of_a_bare_main_names_the_bare_folder() {
    let mut o = pushed();
    o[0] = out(0, "/w/linked\n/w/main.git/worktrees/linked\n/w/main.git\n");
    let f = facts(&o);
    assert_eq!(f.main_repo, Some(PathBuf::from("/w/main.git")));
}

fn origin_of(url: &str) -> Option<String> {
    let mut o = pushed();
    o[3] = out(0, &format!("{url}\n"));
    facts(&o).origin
}

#[test]
fn an_http_origin_never_carries_a_credential() {
    for (url, want) in [
        ("https://user:tok@host/x.git", "https://host/x.git"),
        ("https://tok@host/x.git", "https://host/x.git"),
        ("http://tok@host:8080/x", "http://host:8080/x"),
        ("HTTPS://u:p@Host/x", "HTTPS://Host/x"),
        ("https://u:p@h/a@b.git", "https://h/a@b.git"),
        ("https://host/x.git", "https://host/x.git"),
        ("https://u:p@host", "https://host"),
        ("ssh://git:secret@host/o/r", "ssh://git@host/o/r"),
        ("git+ssh://u:p@h/x", "git+ssh://u@h/x"),
        ("ssh+git://u@h/x", "ssh+git://u@h/x"),
        ("persistent-https://TOKEN@h/x", "persistent-https://h/x"),
        ("ftp://TOKEN@h/x", "ftp://h/x"),
        ("ftps://u:p@h/x", "ftps://h/x"),
        ("git://TOKEN@h/x", "git://h/x"),
        ("codecommit::https://TOKEN@h/x", "codecommit::https://h/x"),
        ("a::b::https://u:p@h/x", "a::b::https://h/x"),
        ("ssh://git@[::1]/x", "ssh://git@[::1]/x"),
        ("git@[::1]:o/r.git", "git@[::1]:o/r.git"),
        ("file:///srv/o/r.git", "file:///srv/o/r.git"),
    ] {
        assert_eq!(origin_of(url).as_deref(), Some(want), "{url}");
    }
}

#[test]
fn ssh_origins_keep_their_user() {
    for url in [
        "git@github.com:o/r.git",
        "ssh://git@host/o/r",
        "/srv/o/r.git",
    ] {
        assert_eq!(origin_of(url).as_deref(), Some(url));
    }
}

fn repo(branch: Option<&str>) -> RepoFacts {
    RepoFacts {
        root: PathBuf::from("/w/repo"),
        origin: Some("o".to_owned()),
        branch: branch.map(str::to_owned),
        head: OID.to_owned(),
        upstream: Some("origin/main".to_owned()),
        unpushed: 0,
        dirty: false,
        untracked: false,
        submodules: false,
        main_repo: None,
    }
}

#[test]
fn every_reason_a_session_stays_has_its_words() {
    assert_eq!(
        movable(None, "/w/x"),
        Movable::No("/w/x isn't a git repository".to_owned())
    );
    let mut f = repo(Some("main"));
    f.dirty = true;
    f.untracked = true;
    f.unpushed = 2;
    let no = |f: &RepoFacts| movable(Some(f), "/w/repo/sub");
    assert_eq!(
        no(&f),
        Movable::No("/w/repo has uncommitted changes".to_owned())
    );
    f.dirty = false;
    assert_eq!(
        no(&f),
        Movable::No("/w/repo has untracked files".to_owned())
    );
    f.untracked = false;
    assert_eq!(
        no(&f),
        Movable::No("main has 2 commits that aren't pushed".to_owned())
    );
    f.unpushed = 1;
    assert_eq!(
        no(&f),
        Movable::No("main has 1 commits that aren't pushed".to_owned())
    );
    f.upstream = None;
    assert_eq!(
        no(&f),
        Movable::No("main has no pushed upstream".to_owned())
    );
    f.upstream = Some("origin/main".to_owned());
    f.origin = None;
    assert_eq!(
        no(&f),
        Movable::No("main has no pushed upstream".to_owned())
    );
    f.branch = None;
    assert_eq!(no(&f), Movable::No("/w/repo isn't on a branch".to_owned()));
}

#[test]
fn dirty_is_reported_before_detached() {
    let mut f = repo(None);
    f.dirty = true;
    assert_eq!(
        movable(Some(&f), "/w/repo"),
        Movable::No("/w/repo has uncommitted changes".to_owned())
    );
}

#[test]
fn facts_and_the_verdict_round_trip_through_json() {
    let f = facts(&pushed());
    let json = serde_json::to_string(&f).unwrap();
    assert_eq!(serde_json::from_str::<RepoFacts>(&json).unwrap(), f);
    let m = Movable::No("x".to_owned());
    let json = serde_json::to_string(&m).unwrap();
    assert_eq!(serde_json::from_str::<Movable>(&json).unwrap(), m);
}

fn flags(args: &[String]) -> Vec<String> {
    args.windows(2)
        .filter(|w| w[0] == "-c")
        .map(|w| w[1].clone())
        .collect()
}

fn after_folder(call: &GitCall) -> Vec<String> {
    let args = call.args();
    let at = args.iter().position(|a| a == "-C").unwrap();
    assert_eq!(args[at + 1], "/w/repo");
    args[at + 2..].to_vec()
}

#[test]
fn every_call_is_hardened() {
    std::env::set_var("GIT_SNAP_REPO_TEST_VAR", "x");
    let calls = read_plan(Path::new("/w/repo"));
    assert_eq!(calls.len(), 5);
    for call in &calls {
        let args = call.args();
        assert_eq!(args[0], "--no-optional-locks");
        let pairs = flags(&args);
        for want in [
            "core.fsmonitor=false",
            "core.hooksPath=/dev/null",
            "diff.external=",
        ] {
            assert!(pairs.iter().any(|p| p == want), "{want} in {args:?}");
        }
        let env = call.env();
        assert_eq!(env.get("GIT_OPTIONAL_LOCKS"), Some(&Some("0".to_owned())));
        assert_eq!(env.get("GIT_TERMINAL_PROMPT"), Some(&Some("0".to_owned())));
        assert_eq!(env.get("GIT_PAGER"), Some(&Some("cat".to_owned())));
        assert_eq!(env.get("LC_ALL"), Some(&Some("C".to_owned())));
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_SNAP_REPO_TEST_VAR",
        ] {
            assert_eq!(env.get(key), Some(&None), "{key} cleared");
        }
        let command = call.to_command();
        assert_eq!(command.argv[0], "git");
        assert_eq!(command.argv[1..], args[..]);
        assert_eq!(command.env, env);
    }
}

#[test]
fn the_plan_asks_for_what_the_parser_reads() {
    let calls = read_plan(Path::new("/w/repo"));
    assert_eq!(
        after_folder(&calls[0]),
        [
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir"
        ]
    );
    assert_eq!(
        after_folder(&calls[1]),
        [
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=normal",
            "--ignore-submodules=all"
        ]
    );
    assert_eq!(
        after_folder(&calls[2]),
        ["rev-list", "--count", "@{upstream}..HEAD"]
    );
    assert_eq!(after_folder(&calls[3]), ["remote", "get-url", "origin"]);
    assert_eq!(after_folder(&calls[4]), ["ls-files", "-z", "-s", "-v"]);
}

#[test]
fn the_filter_probe_is_a_hardened_config_read() {
    let probe = filter_probe(Path::new("/w/repo"));
    assert_eq!(probe.args()[0], "--no-optional-locks");
    assert_eq!(
        after_folder(&probe),
        ["config", "-z", "--get-regexp", "^filter\\."]
    );
    assert_eq!(
        probe.env().get("GIT_OPTIONAL_LOCKS"),
        Some(&Some("0".to_owned()))
    );
}

#[test]
fn filters_are_blanked_on_every_call() {
    let probe = "filter.x.clean\nscript\0filter.a.b.smudge\ncat\0filter.x.required\ntrue\0";
    let calls = without_filters(read_plan(Path::new("/w/repo")), probe).unwrap();
    assert_eq!(calls.len(), 5);
    for call in &calls {
        let args = call.args();
        let pairs = flags(&args);
        for want in [
            "filter.x.clean=",
            "filter.x.smudge=",
            "filter.x.process=",
            "filter.x.required=false",
            "filter.a.b.clean=",
            "filter.a.b.required=false",
        ] {
            assert!(pairs.iter().any(|p| p == want), "{want}");
        }
        let last = args.iter().rposition(|a| a == "-c").unwrap();
        let dir = args.iter().position(|a| a == "-C").unwrap();
        assert!(last < dir, "config flags come before -C");
    }
}

#[test]
fn no_filters_changes_nothing() {
    let plan = read_plan(Path::new("/w/repo"));
    assert_eq!(without_filters(plan.clone(), "").unwrap(), plan);
}

#[test]
fn a_filter_name_git_cant_be_told_to_skip_is_refused() {
    let plan = read_plan(Path::new("/w/repo"));
    assert!(without_filters(plan, "filter.a=b.clean\nx\0").is_err());
}
