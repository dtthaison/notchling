use crate::acting_ops::now_ms;
use crate::git_ops::lock;
use crate::git_testkit::{RepoRig, Seen};
use crate::inbox_ops_tests::app;
use crate::op_group::OpGroup;
use notchling_engine::git::reader::Reader;
use notchling_local_api::ops;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const ALLOWED: [&str; 10] = [
    "config",
    "status",
    "diff",
    "rev-parse",
    "merge-base",
    "symbolic-ref",
    "for-each-ref",
    "worktree",
    "rev-list",
    "log",
];
const NEVER: [&str; 6] = ["fetch", "pull", "push", "remote", "ls-remote", "gc"];
const DIFF_FLAGS: [&str; 5] = [
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--ignore-submodules=all",
    "-M",
];

fn rest(seen: &Seen) -> &[String] {
    let at = seen.argv.iter().position(|a| a == "-C").unwrap();
    &seen.argv[at + 3..]
}

fn forms(rig: &RepoRig) -> Vec<(&'static str, Value)> {
    let repo = rig.cwd();
    let lane = rig.lane_a.to_string_lossy().into_owned();
    let heads = "refs/heads/lane-a";
    let far = "refs/remotes/origin/far";
    let mut all: Vec<(&'static str, Value)> = Vec::new();
    for cwd in [&repo, &lane] {
        all.push((ops::GIT_STATUS, json!({"cwd": cwd})));
        all.push((ops::GIT_DIFF, json!({"cwd": cwd, "path": "untracked.txt"})));
        all.push((ops::GIT_BRANCHES, json!({"cwd": cwd})));
        all.push((ops::GIT_BRANCHES, json!({"cwd": cwd, "worktree": lane})));
        all.push((ops::GIT_WORKTREES, json!({"cwd": cwd})));
        all.push((ops::GIT_WORKTREES, json!({"cwd": cwd, "fresh": [lane]})));
        all.push((ops::GIT_BRANCH_LIST, json!({"cwd": cwd})));
        all.push((ops::GIT_BRANCH_DIFF_STAT, json!({"cwd": cwd})));
        all.push((
            ops::GIT_BRANCH_DIFF_STAT,
            json!({"cwd": cwd, "base": "int"}),
        ));
        all.push((
            ops::GIT_BRANCH_DIFF_STAT,
            json!({"cwd": cwd, "pick": "nearest"}),
        ));
        all.push((
            ops::GIT_BRANCH_DIFF_STAT,
            json!({"cwd": cwd, "worktree": lane, "base": "main"}),
        ));
        all.push((
            ops::GIT_BRANCH_DIFF_STAT,
            json!({"cwd": cwd, "worktree": lane, "pick": "nearest"}),
        ));
        for name in [heads, far] {
            all.push((ops::GIT_BRANCH_DIFF_STAT, json!({"cwd": cwd, "ref": name})));
            all.push((
                ops::GIT_BRANCH_DIFF_STAT,
                json!({"cwd": cwd, "ref": name, "base": "main"}),
            ));
            all.push((
                ops::GIT_BRANCH_DIFF_STAT,
                json!({"cwd": cwd, "ref": name, "pick": "nearest"}),
            ));
        }
        all.push((
            ops::GIT_BRANCH_DIFF_FILE,
            json!({"cwd": cwd, "base": "int", "path": "untracked.txt", "worktree": lane}),
        ));
        all.push((
            ops::GIT_BRANCH_DIFF_FILE,
            json!({"cwd": cwd, "base": "main", "path": "i1.txt"}),
        ));
        all.push((
            ops::GIT_BRANCH_DIFF_FILE,
            json!({"cwd": cwd, "base": "int", "path": "l1.txt", "ref": heads}),
        ));
    }
    all
}

fn run_everything(old_git: bool) -> Vec<Seen> {
    let rig = RepoRig::new("allow");
    rig.launcher.old_git(old_git);
    let reader = Reader::open(&rig.cwd(), &*rig.launcher).unwrap();
    let key = reader.common_dir().unwrap();
    let mut ok = 0;
    for (op, args) in forms(&rig) {
        if rig.ops.call(&app(), op, &args).is_ok() {
            ok += 1;
        }
    }
    assert!(ok >= 30, "only {ok} forms answered");
    let end = Instant::now() + Duration::from_secs(30);
    while !lock(&rig.ops.cache).repo(&key, now_ms()).reading.is_empty() {
        assert!(Instant::now() < end, "reads never finished");
        std::thread::sleep(Duration::from_millis(20));
    }
    rig.launcher.seen()
}

#[test]
fn every_git_command_is_in_the_allowlist() {
    let mut seen = run_everything(false);
    seen.extend(run_everything(true));
    assert!(seen.len() > 100);
    for one in &seen {
        assert!(ALLOWED.contains(&one.sub.as_str()), "{:?}", one.argv);
        assert!(!NEVER.contains(&one.sub.as_str()), "{:?}", one.argv);
        let args = rest(one);
        match one.sub.as_str() {
            "worktree" => assert_eq!(args.first().map(String::as_str), Some("list")),
            "config" => {
                assert_eq!(args[0], "-z", "{:?}", one.argv);
                assert_eq!(args[1], "--get-regexp", "{:?}", one.argv);
                assert_eq!(args.len(), 3, "{:?}", one.argv);
            }
            _ => {}
        }
    }
    for sub in [
        "config",
        "status",
        "diff",
        "rev-parse",
        "merge-base",
        "for-each-ref",
        "worktree",
        "rev-list",
        "log",
    ] {
        assert!(seen.iter().any(|s| s.sub == sub), "never ran {sub}");
    }
}

#[test]
fn diff_shortstat_carries_every_diff_flag() {
    let seen = run_everything(false);
    let shortstats: Vec<&Seen> = seen
        .iter()
        .filter(|s| s.sub == "diff" && s.argv.iter().any(|a| a == "--shortstat"))
        .collect();
    assert!(!shortstats.is_empty());
    for one in shortstats {
        for flag in DIFF_FLAGS {
            assert!(
                one.argv.iter().any(|a| a == flag),
                "{flag} in {:?}",
                one.argv
            );
        }
        assert_eq!(
            one.argv.last().map(String::as_str),
            Some("--"),
            "{:?}",
            one.argv
        );
    }
}
