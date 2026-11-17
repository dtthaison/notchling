use crate::acting_ops::now_ms;
use crate::git_ops::lock;
use crate::git_testkit::{git_out, RepoRig};
use crate::inbox_ops_tests::app;
use crate::op_group::OpGroup;
use notchling_engine::git::reader::Reader;
use notchling_engine::git::{read_branch_diff, read_branch_stat, read_branches};
use notchling_local_api::{ops, ApiError, ErrorCode};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

fn call(rig: &RepoRig, op: &str, cwd: &str, extra: Value) -> Result<Value, ApiError> {
    let mut args = json!({ "cwd": cwd });
    if let Some(map) = extra.as_object() {
        for (key, value) in map {
            args[key] = value.clone();
        }
    }
    rig.ops.call(&app(), op, &args)
}

fn lane(rig: &RepoRig) -> String {
    rig.lane_a.to_string_lossy().into_owned()
}

fn stat(rig: &RepoRig, cwd: &str, extra: Value) -> Value {
    call(rig, ops::GIT_BRANCH_DIFF_STAT, cwd, extra).unwrap()
}

fn paths(stat: &Value) -> Vec<String> {
    let mut found: Vec<String> = stat["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_owned())
        .collect();
    found.sort();
    found
}

fn key_of(rig: &RepoRig) -> std::path::PathBuf {
    let reader = Reader::open(&rig.cwd(), &*rig.launcher).unwrap();
    let key = reader.common_dir().unwrap();
    rig.launcher.clear();
    key
}

fn idle(rig: &RepoRig, key: &Path) {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        if lock(&rig.ops.cache).repo(key, now_ms()).reading.is_empty() {
            return;
        }
        assert!(Instant::now() < end, "reads never finished");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn stat_in_another_worktree_reads_there() {
    let rig = RepoRig::new("tg-worktree");
    let got = stat(
        &rig,
        &rig.cwd(),
        json!({"worktree": lane(&rig), "base": "int"}),
    );
    assert_eq!(got["repo"], true);
    assert_eq!(got["branch"], "lane-a");
    assert_eq!(got["base"], "int");
    assert_eq!(got["includes_uncommitted"], true);
    assert_eq!(paths(&got), ["l1.txt", "l2.txt", "untracked.txt"]);
    assert_eq!(rig.launcher.count_at("status", "lane-a"), 1);
    assert_eq!(rig.launcher.count_at("status", "/repo"), 0);
}

#[test]
fn stat_pick_nearest_names_the_base() {
    let rig = RepoRig::new("tg-nearest");
    let got = stat(&rig, &lane(&rig), json!({"pick": "nearest"}));
    assert_eq!(got["base"], "int");
    assert_eq!(got["includes_uncommitted"], true);
    assert_eq!(paths(&got), ["l1.txt", "l2.txt", "untracked.txt"]);
    let main = stat(&rig, &rig.cwd(), json!({"pick": "nearest"}));
    assert_eq!(main["base"], "main");
}

#[test]
fn explicit_base_wins_over_pick() {
    let rig = RepoRig::new("tg-explicit");
    let got = stat(
        &rig,
        &lane(&rig),
        json!({"pick": "nearest", "base": "main"}),
    );
    assert_eq!(got["base"], "main");
    assert!(paths(&got).contains(&"i1.txt".to_owned()));
}

#[test]
fn pick_nearest_skips_an_unrelated_candidate() {
    let rig = RepoRig::new("tg-skip");
    let key = key_of(&rig);
    let int = git_out(&rig.repo, &["rev-parse", "int"]);
    let head = git_out(&rig.repo, &["rev-parse", "lane-a"]);
    lock(&rig.ops.cache)
        .repo(&key, now_ms())
        .numbers
        .put_related(&int, &head, false);
    let got = stat(&rig, &lane(&rig), json!({"pick": "nearest"}));
    assert_eq!(got["base"], "main");
    assert!(paths(&got).contains(&"i1.txt".to_owned()));
}

#[test]
fn pick_nearest_with_no_related_base_has_none() {
    let rig = RepoRig::new("tg-orphan");
    let orphan = rig.lane("lane-o", &[], &["orphan"]);
    let got = stat(&rig, &orphan.to_string_lossy(), json!({"pick": "nearest"}));
    assert_eq!(got["repo"], true);
    assert_eq!(got["base"], Value::Null);
    assert!(paths(&got).is_empty());
}

#[test]
fn pick_nearest_cached_runs_no_rev_list() {
    let rig = RepoRig::new("tg-cached");
    stat(&rig, &lane(&rig), json!({"pick": "nearest"}));
    rig.launcher.clear();
    let again = stat(&rig, &lane(&rig), json!({"pick": "nearest"}));
    assert_eq!(again["base"], "int");
    assert_eq!(rig.launcher.count("rev-list"), 0);
    assert_eq!(rig.launcher.count("for-each-ref"), 1);
    assert_eq!(rig.launcher.count("worktree"), 0);
}

#[test]
fn no_new_args_reply_unchanged() {
    let rig = RepoRig::new("tg-plain");
    let cwd = lane(&rig);
    let mut want =
        serde_json::to_value(read_branch_stat(&cwd, Some("int"), &*rig.launcher).unwrap()).unwrap();
    want["repo"] = json!(true);
    want["includes_uncommitted"] = json!(true);
    let got = stat(&rig, &cwd, json!({"base": "int"}));
    assert_eq!(got.to_string(), want.to_string());

    let mut default =
        serde_json::to_value(read_branch_stat(&cwd, None, &*rig.launcher).unwrap()).unwrap();
    default["repo"] = json!(true);
    default["includes_uncommitted"] = json!(true);
    assert_eq!(stat(&rig, &cwd, json!({})).to_string(), default.to_string());

    let mut branches = serde_json::to_value(read_branches(&cwd, &*rig.launcher).unwrap()).unwrap();
    branches["repo"] = json!(true);
    let got = call(&rig, ops::GIT_BRANCHES, &cwd, json!({})).unwrap();
    assert_eq!(got.to_string(), branches.to_string());

    let file = read_branch_diff(&cwd, "int", "l1.txt", None, &*rig.launcher).unwrap();
    let got = call(
        &rig,
        ops::GIT_BRANCH_DIFF_FILE,
        &cwd,
        json!({"base": "int", "path": "l1.txt"}),
    )
    .unwrap();
    assert_eq!(
        got.to_string(),
        serde_json::to_value(file).unwrap().to_string()
    );
}

#[test]
fn ref_stat_runs_no_status() {
    let rig = RepoRig::new("tg-ref-status");
    let got = stat(
        &rig,
        &rig.cwd(),
        json!({"ref": "refs/heads/lane-a", "base": "int"}),
    );
    assert_eq!(rig.launcher.count("status"), 0);
    assert_eq!(paths(&got), ["l1.txt", "l2.txt"]);
    assert_eq!(got["base"], "int");
}

#[test]
fn ref_before_double_dash() {
    let rig = RepoRig::new("tg-ref-dash");
    let name = "refs/heads/lane-a";
    stat(&rig, &rig.cwd(), json!({"ref": name, "base": "int"}));
    stat(&rig, &rig.cwd(), json!({"ref": name, "pick": "nearest"}));
    let file = call(
        &rig,
        ops::GIT_BRANCH_DIFF_FILE,
        &rig.cwd(),
        json!({"ref": name, "base": "int", "path": "l1.txt"}),
    );
    assert!(file.is_ok());
    let mut with_ref = 0;
    for seen in rig.launcher.seen() {
        let Some(at) = seen.argv.iter().position(|a| a.starts_with(name)) else {
            continue;
        };
        with_ref += 1;
        assert!(
            ["merge-base", "diff", "rev-parse"].contains(&seen.sub.as_str()),
            "{:?}",
            seen.argv
        );
        if let Some(dash) = seen.argv.iter().position(|a| a == "--") {
            assert!(at < dash, "{:?}", seen.argv);
        }
        assert!(!seen.argv[at].starts_with('-'));
    }
    assert!(with_ref >= 4);
}

#[test]
fn ref_stat_reply_fields() {
    let rig = RepoRig::new("tg-ref-fields");
    let got = stat(
        &rig,
        &rig.cwd(),
        json!({"ref": "refs/heads/lane-a", "base": "int"}),
    );
    let tip = git_out(&rig.repo, &["rev-parse", "lane-a"]);
    assert_eq!(got["repo"], true);
    assert_eq!(got["branch"], "lane-a");
    assert_eq!(got["detached"], false);
    assert_eq!(got["head"], &tip[..7]);
    assert_eq!(got["includes_uncommitted"], false);
    assert_eq!(got["uncommitted_files"], 0);
    assert_eq!(got["file_count"], 2);
    assert_eq!(got["merge_base"].as_str().unwrap().len(), 7);
    let remote = stat(
        &rig,
        &rig.cwd(),
        json!({"ref": "refs/remotes/origin/far", "base": "main"}),
    );
    assert_eq!(remote["branch"], "origin/far");
    assert_eq!(remote["includes_uncommitted"], false);
    let bare = stat(&rig, &rig.cwd(), json!({"ref": "refs/heads/lane-a"}));
    assert_eq!(bare["base"], Value::Null);
    assert_eq!(bare["includes_uncommitted"], false);
}

#[test]
fn ref_file_path_must_be_in_the_stat() {
    let rig = RepoRig::new("tg-ref-file");
    let ask = |path: &str| {
        call(
            &rig,
            ops::GIT_BRANCH_DIFF_FILE,
            &rig.cwd(),
            json!({"ref": "refs/heads/lane-a", "base": "int", "path": path}),
        )
    };
    let got = ask("l1.txt").unwrap();
    assert_eq!(got["path"], "l1.txt");
    assert!(got["text"].as_str().unwrap().contains("+x"));
    assert_eq!(ask("a.txt").unwrap_err().code, ErrorCode::Forbidden);
    assert_eq!(ask("untracked.txt").unwrap_err().code, ErrorCode::Forbidden);
    let hostile = call(
        &rig,
        ops::GIT_BRANCH_DIFF_FILE,
        &rig.cwd(),
        json!({"ref": "refs/heads/nope", "base": "int", "path": "l1.txt"}),
    );
    assert_eq!(hostile.unwrap_err().code, ErrorCode::Forbidden);
}

#[test]
fn branches_in_a_worktree() {
    let rig = RepoRig::new("tg-branches");
    let here = call(&rig, ops::GIT_BRANCHES, &rig.cwd(), json!({})).unwrap();
    assert_eq!(here["current"], "int");
    let there = call(
        &rig,
        ops::GIT_BRANCHES,
        &rig.cwd(),
        json!({"worktree": lane(&rig)}),
    )
    .unwrap();
    assert_eq!(there["current"], "lane-a");
    assert_eq!(there["repo"], true);
    let outside = call(
        &rig,
        ops::GIT_BRANCHES,
        &rig.cwd(),
        json!({"worktree": rig.t.root().to_string_lossy()}),
    );
    assert_eq!(outside.unwrap_err().code, ErrorCode::Forbidden);
}

#[test]
fn worktree_and_ref_together_refused() {
    let rig = RepoRig::new("tg-both");
    let both = json!({"worktree": lane(&rig), "ref": "refs/heads/lane-a", "base": "int", "path": "l1.txt"});
    for op in [
        ops::GIT_BRANCH_DIFF_STAT,
        ops::GIT_BRANCH_DIFF_FILE,
        ops::GIT_BRANCHES,
    ] {
        let err = call(&rig, op, &rig.cwd(), both.clone());
        let ref_less = op == ops::GIT_BRANCHES;
        if ref_less {
            assert!(err.is_ok(), "{op}");
        } else {
            assert_eq!(err.unwrap_err().code, ErrorCode::BadRequest, "{op}");
        }
    }
    assert_eq!(rig.launcher.count("status"), 0);
}

#[test]
fn nearest_stat_fills_work_cache() {
    let rig = RepoRig::new("tg-work");
    let real = std::fs::canonicalize(&rig.lane_a).unwrap();
    assert!(rig.ops.cached_work(&real).is_none());
    stat(&rig, &lane(&rig), json!({"base": "int"}));
    assert!(rig.ops.cached_work(&real).is_none());
    stat(
        &rig,
        &lane(&rig),
        json!({"pick": "nearest", "base": "main"}),
    );
    assert!(rig.ops.cached_work(&real).is_none());
    let got = stat(&rig, &lane(&rig), json!({"pick": "nearest"}));
    let work = rig.ops.cached_work(&real).unwrap();
    assert_eq!(work.files, 3);
    assert_eq!(work.uncommitted_files, 1);
    assert_eq!(work.added, got["added"].as_u64().unwrap());
    assert_eq!(work.removed, got["removed"].as_u64().unwrap());
    assert!(work.read_at > 0);
    let other = stat(
        &rig,
        &rig.cwd(),
        json!({"worktree": lane(&rig), "pick": "nearest"}),
    );
    assert_eq!(other["base"], "int");
    let key = key_of(&rig);
    call(&rig, ops::GIT_BRANCH_LIST, &lane(&rig), json!({})).unwrap();
    idle(&rig, &key);
    assert_eq!(rig.launcher.count_at("status", "lane-a"), 0);
}

#[test]
fn status_has_changed_at() {
    let rig = RepoRig::new("tg-changed-at");
    let got = call(&rig, ops::GIT_STATUS, &lane(&rig), json!({})).unwrap();
    assert!(got["changed_at"].as_i64().unwrap() > 0);
    let clean = call(&rig, ops::GIT_STATUS, &rig.cwd(), json!({})).unwrap();
    assert!(clean.get("changed_at").is_none());
}
