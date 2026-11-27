use crate::acting_ops::now_ms;
use crate::git_ops::{lock, GIT_OPS};
use crate::git_testkit::{git, git_out, RepoRig};
use crate::inbox_ops_tests::app;
use crate::op_group::OpGroup;
use notchling_engine::git::reader::Reader;
use notchling_engine::git::refs::Counts;
use notchling_local_api::{ops, ApiError, OpSpec};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn ask(rig: &RepoRig, cwd: &str) -> Result<Value, ApiError> {
    rig.ops
        .call(&app(), ops::GIT_BRANCH_LIST, &json!({ "cwd": cwd }))
}

pub(crate) fn reply(rig: &RepoRig) -> Value {
    ask(rig, &rig.cwd()).unwrap()
}

pub(crate) fn key_of(rig: &RepoRig) -> PathBuf {
    let reader = Reader::open(&rig.cwd(), &*rig.launcher).unwrap();
    let key = reader.common_dir().unwrap();
    rig.launcher.clear();
    key
}

pub(crate) fn idle(rig: &RepoRig, key: &Path) {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let reading = lock(&rig.ops.cache).repo(key, now_ms()).reading.len();
        if reading == 0 {
            return;
        }
        assert!(Instant::now() < end, "reads never finished");
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn branches(reply: &Value) -> &Vec<Value> {
    reply["branches"].as_array().unwrap()
}

fn row<'a>(reply: &'a Value, name: &str) -> &'a Value {
    branches(reply)
        .iter()
        .find(|b| b["name"] == name)
        .unwrap_or_else(|| panic!("no row {name} in {reply}"))
}

fn settled(rig: &RepoRig) -> Value {
    let key = key_of(rig);
    for _ in 0..10 {
        let got = reply(rig);
        idle(rig, &key);
        let waiting = branches(&got).iter().any(|b| {
            b["pending"] == true
                || b["worktree"].as_object().is_some_and(|w| {
                    w["work"].is_null() && w["missing"] == false && !b["base"].is_null()
                })
        });
        if !waiting {
            rig.launcher.clear();
            return got;
        }
    }
    panic!("never settled");
}

pub(crate) fn shortstats(rig: &RepoRig) -> usize {
    rig.launcher
        .seen()
        .iter()
        .filter(|s| s.sub == "diff" && s.argv.iter().any(|a| a == "--shortstat"))
        .count()
}

fn commit_on(rig: &RepoRig, parent: &str, message: &str) -> String {
    let tree = format!("{parent}^{{tree}}");
    git_out(
        &rig.repo,
        &["commit-tree", &tree, "-p", parent, "-m", message],
    )
}

#[test]
fn the_op_is_app_quiet_and_in_git_ops() {
    let rig = RepoRig::new("bl-spec");
    assert_eq!(rig.ops.spec(ops::GIT_BRANCH_LIST), Some(OpSpec::APP_QUIET));
    assert!(GIT_OPS.contains(&ops::GIT_BRANCH_LIST));
}

#[test]
fn one_row_per_local_and_remote_only() {
    let rig = RepoRig::new("bl-rows");
    let got = reply(&rig);
    assert_eq!(got["repo"], true);
    assert_eq!(got["name"], "repo");
    assert_eq!(got["default"], "main");
    assert_eq!(got["integration"], "int");
    assert_eq!(got["truncated"], false);
    assert!(got["fetched_at"].as_i64().is_some());
    assert_eq!(branches(&got).len(), 7);
    let local = row(&got, "lane-a");
    assert_eq!(local["local"], true);
    assert_eq!(local["remote"], Value::Null);
    assert_eq!(local["remote_only"], false);
    assert_eq!(local["ref"], "refs/heads/lane-a");
    let far = row(&got, "far");
    assert_eq!(far["local"], false);
    assert_eq!(far["remote"], "origin");
    assert_eq!(far["remote_only"], true);
    assert_eq!(far["ref"], "refs/remotes/origin/far");
    let detached = branches(&got).iter().find(|b| b["detached"] == true);
    assert!(detached.is_some_and(|b| b["name"].is_null()));
}

#[test]
fn origin_head_skipped_and_remote_duplicate_dropped() {
    let rig = RepoRig::new("bl-dup");
    git(
        &rig.repo,
        &["update-ref", "refs/remotes/origin/HEAD", "int"],
    );
    git(&rig.repo, &["update-ref", "refs/remotes/origin/int", "int"]);
    git(
        &rig.repo,
        &["update-ref", "refs/remotes/origin/lane-a", "lane-a"],
    );
    let got = reply(&rig);
    assert!(branches(&got).iter().all(|b| b["name"] != "HEAD"));
    for name in ["int", "lane-a"] {
        let rows: Vec<&Value> = branches(&got)
            .iter()
            .filter(|b| b["name"] == name)
            .collect();
        assert_eq!(rows.len(), 1, "{name}");
        assert_eq!(rows[0]["local"], true);
    }
    assert_eq!(branches(&got).len(), 7);
}

#[test]
fn bases_never_classified() {
    let rig = RepoRig::new("bl-bases");
    let got = settled(&rig);
    for name in ["main", "int"] {
        let base = row(&got, name);
        assert_eq!(base["is_base"], true, "{name}");
        assert_eq!(base["merged_into"], Value::Null, "{name}");
        assert_eq!(base["base"], Value::Null, "{name}");
        assert_eq!(base["lines"], Value::Null, "{name}");
        assert_eq!(base["pending"], false, "{name}");
        assert_eq!(base["unrelated"], false, "{name}");
    }
    assert_eq!(row(&got, "lane-a")["is_base"], false);
}

#[test]
fn merged_into_integration_or_default() {
    let rig = RepoRig::new("bl-merged");
    let main_two = commit_on(&rig, "main", "main two");
    git(&rig.repo, &["update-ref", "refs/heads/main", &main_two]);
    git(&rig.repo, &["branch", "viamain", &main_two]);
    let got = settled(&rig);
    assert_eq!(row(&got, "merged")["merged_into"], "int");
    assert_eq!(row(&got, "viamain")["merged_into"], "main");
    assert_eq!(row(&got, "lane-a")["merged_into"], Value::Null);
    assert_eq!(row(&got, "far")["merged_into"], "int");
}

#[test]
fn never_merged_into_own_short_name() {
    let rig = RepoRig::new("bl-own");
    git(
        &rig.repo,
        &["update-ref", "refs/remotes/origin/main", "main"],
    );
    git(
        &rig.repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    let got = settled(&rig);
    assert_eq!(got["default"], "origin/main");
    let main = row(&got, "main");
    assert_eq!(main["is_base"], true);
    assert_eq!(main["merged_into"], Value::Null);
    assert_eq!(main["base"], Value::Null);
    assert_eq!(
        branches(&got)
            .iter()
            .filter(|b| b["name"] == "main")
            .count(),
        1
    );
    assert_eq!(row(&got, "merged")["merged_into"], "int");
}

#[test]
fn lines_null_for_merged_and_bases() {
    let rig = RepoRig::new("bl-lines");
    let got = settled(&rig);
    assert_eq!(row(&got, "merged")["lines"], Value::Null);
    assert_eq!(row(&got, "main")["lines"], Value::Null);
    let lane = row(&got, "lane-a");
    assert_eq!(lane["base"], "int");
    assert_eq!(lane["ahead"], 2);
    assert_eq!(lane["merged_into"], Value::Null);
    assert_eq!(lane["lines"], json!({"added": 3, "removed": 0, "files": 2}));
    assert_eq!(lane["pending"], false);
}

#[test]
fn lines_pending_past_one_second_budget() {
    let rig = RepoRig::new("bl-budget");
    let int = git_out(&rig.repo, &["rev-parse", "int"]);
    for n in 0..4 {
        let tip = commit_on(&rig, &int, &format!("tip {n}"));
        git(&rig.repo, &["branch", &format!("tip-{n}"), &tip]);
    }
    let key = key_of(&rig);
    rig.launcher.delay("diff", Some(Duration::from_millis(400)));
    let started = Instant::now();
    let got = reply(&rig);
    assert!(started.elapsed() < Duration::from_secs(8));
    idle(&rig, &key);
    let waiting = branches(&got)
        .iter()
        .filter(|b| b["pending"] == true)
        .count();
    assert!(waiting > 0, "{got}");
    assert_eq!(got["pending"], waiting);
    assert!(shortstats(&rig) <= 3, "{} reads", shortstats(&rig));
    rig.launcher.delay("diff", None);
    let mut left = waiting;
    for _ in 0..10 {
        if left == 0 {
            break;
        }
        let next = reply(&rig);
        idle(&rig, &key);
        let now = next["pending"].as_u64().unwrap() as usize;
        assert!(now < left || now == 0, "no progress: {left} then {now}");
        left = now;
    }
    assert_eq!(left, 0);
}

#[test]
fn upstream_only_candidate_settles() {
    let rig = RepoRig::new("bl-upstream-only");
    let int = git_out(&rig.repo, &["rev-parse", "int"]);
    let remote_tip = commit_on(&rig, &int, "remote tip");
    git(
        &rig.repo,
        &["update-ref", "refs/remotes/origin/int", &remote_tip],
    );
    let topic_tip = commit_on(&rig, &int, "topic tip");
    git(&rig.repo, &["branch", "topic", &topic_tip]);
    git(
        &rig.repo,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );
    git(&rig.repo, &["config", "branch.topic.remote", "origin"]);
    git(
        &rig.repo,
        &["config", "branch.topic.merge", "refs/heads/int"],
    );
    let key = key_of(&rig);
    reply(&rig);
    idle(&rig, &key);
    let last = reply(&rig);
    idle(&rig, &key);
    let topic = row(&last, "topic");
    assert_eq!(topic["upstream"], "origin/int", "{topic}");
    assert_eq!(topic["pending"], false, "{topic}");
    assert!(!topic["base"].is_null(), "{topic}");
    assert_eq!(last["pending"], 0, "{last}");
}

#[test]
fn prunable_detached_head_is_not_pending_forever() {
    let rig = RepoRig::new("bl-prunable");
    let int = git_out(&rig.repo, &["rev-parse", "int"]);
    let tip = commit_on(&rig, &int, "gone tip");
    let dir = rig.lane("gone", &["--detach"], &[&tip]);
    std::fs::remove_dir_all(&dir).unwrap();
    let key = key_of(&rig);
    reply(&rig);
    idle(&rig, &key);
    let last = reply(&rig);
    idle(&rig, &key);
    let gone = branches(&last)
        .iter()
        .find(|b| b["detached"] == true && b["subject"].is_null())
        .unwrap_or_else(|| panic!("no prunable row in {last}"));
    assert_eq!(gone["pending"], false, "{gone}");
    assert_eq!(last["pending"], 0, "{last}");
}

#[test]
fn second_call_same_heads_runs_no_diff_or_rev_list() {
    let rig = RepoRig::new("bl-cached");
    let first = settled(&rig);
    let second = reply(&rig);
    assert_eq!(rig.launcher.count("diff"), 0);
    assert_eq!(rig.launcher.count("rev-list"), 0);
    assert_eq!(rig.launcher.count("status"), 0);
    assert_eq!(rig.launcher.count("merge-base"), 0);
    assert_eq!(
        row(&second, "lane-a")["lines"],
        row(&first, "lane-a")["lines"]
    );
    assert_eq!(second["pending"], 0);
}

#[test]
fn worktree_rows_carry_work_from_the_cache() {
    let rig = RepoRig::new("bl-work");
    let got = settled(&rig);
    let lane = &row(&got, "lane-a")["worktree"];
    assert!(lane["path"].as_str().unwrap().ends_with("lane-a"));
    assert_eq!(lane["current"], false);
    assert_eq!(lane["work"]["files"], 3);
    assert_eq!(lane["work"]["uncommitted_files"], 1);
    assert!(lane["work"]["read_at"].as_i64().is_some());
    let main = &row(&got, "int")["worktree"];
    assert_eq!(main["main"], true);
    assert_eq!(main["current"], true);
    assert!(!main["work"].is_null(), "the current worktree is read too");
    assert_eq!(row(&got, "merged")["worktree"], Value::Null);
}

#[test]
fn unrelated_branch_has_no_base() {
    let rig = RepoRig::new("bl-unrelated");
    let got = reply(&rig);
    let orphan = row(&got, "orphan");
    assert_eq!(orphan["base"], Value::Null);
    assert_eq!(orphan["ahead"], Value::Null);
    assert_eq!(orphan["unrelated"], true);
    assert_eq!(orphan["pending"], false);
    assert_eq!(orphan["lines"], Value::Null);
    assert_eq!(orphan["merged_into"], Value::Null);
    assert!(shortstats(&rig) >= 2);
    let again = reply(&rig);
    assert_eq!(row(&again, "orphan")["unrelated"], true);
}

#[test]
fn detached_worktree_row_has_its_commit_time_and_subject() {
    let rig = RepoRig::new("bl-detached");
    let got = settled(&rig);
    let detached = branches(&got)
        .iter()
        .find(|b| b["detached"] == true)
        .unwrap();
    assert_eq!(detached["name"], Value::Null);
    assert_eq!(detached["head"].as_str().unwrap().len(), 7);
    assert_eq!(detached["subject"], "int three");
    assert!(detached["committed_at"].as_i64().is_some());
    assert!(detached["worktree"]["path"]
        .as_str()
        .unwrap()
        .ends_with("lane-b"));
    assert_eq!(detached["base"], "int");
    assert_eq!(detached["merged_into"], "int");
}

#[test]
fn on_github_from_host() {
    let rig = RepoRig::new("bl-github");
    git(
        &rig.repo,
        &["config", "remote.other.url", "https://gitlab.com/o/r.git"],
    );
    git(&rig.repo, &["update-ref", "refs/remotes/other/side", "int"]);
    let got = reply(&rig);
    let far = row(&got, "far");
    assert_eq!(
        (far["on_github"].clone(), far["remote"].clone()),
        (json!(true), json!("origin"))
    );
    let side = row(&got, "side");
    assert_eq!(
        (side["on_github"].clone(), side["remote"].clone()),
        (json!(false), json!("other"))
    );
    assert_eq!(row(&got, "lane-a")["on_github"], false);
}

#[test]
fn subject_plain_and_cut() {
    let rig = RepoRig::new("bl-subject");
    let long = "x".repeat(500);
    let text = format!("a\u{7}b\u{1b}[31m\u{202e}c {long}");
    let tip = commit_on(&rig, "int", &text);
    git(&rig.repo, &["branch", "weird", &tip]);
    let got = reply(&rig);
    let subject = row(&got, "weird")["subject"].as_str().unwrap().to_owned();
    assert!(subject.chars().all(|c| !c.is_control() && c != '\u{202e}'));
    assert!(subject.starts_with("ab[31mc x"));
    assert_eq!(subject.chars().count(), 120);
    assert_eq!(row(&got, "lane-a")["subject"], "lane two");
}

#[test]
fn remote_url_never_in_reply_or_audit() {
    let rig = RepoRig::new("bl-url");
    let got = settled(&rig);
    let mut texts = vec![got.to_string()];
    texts.push(
        rig.ops
            .call(&app(), ops::GIT_WORKTREES, &json!({"cwd": rig.cwd()}))
            .unwrap()
            .to_string(),
    );
    let outside = rig.t.path("elsewhere").to_string_lossy().into_owned();
    let calls = [
        (ops::GIT_BRANCH_LIST, json!({"cwd": outside})),
        (ops::GIT_BRANCH_LIST, json!({})),
        (ops::GIT_WORKTREES, json!({"cwd": outside})),
        (ops::GIT_DIFF, json!({"cwd": rig.cwd(), "path": "../../x"})),
        (
            ops::GIT_BRANCH_DIFF_STAT,
            json!({"cwd": rig.cwd(), "base": "-c"}),
        ),
        (
            ops::GIT_BRANCH_DIFF_STAT,
            json!({"cwd": rig.cwd(), "base": "nope"}),
        ),
        (
            ops::GIT_BRANCH_DIFF_FILE,
            json!({"cwd": rig.cwd(), "base": "main", "path": "nope.txt"}),
        ),
    ];
    for (op, args) in calls {
        match rig.ops.call(&app(), op, &args) {
            Ok(value) => texts.push(value.to_string()),
            Err(err) => texts.push(format!("{err:?} {}", err.message)),
        }
    }
    for text in texts {
        for secret in ["secret", "me:", "o/r.git", "github.com/o"] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
    }
}

fn counts_of(rig: &RepoRig, key: &Path, base: &str, head: &str) -> Option<Option<Counts>> {
    lock(&rig.ops.cache)
        .repo(key, now_ms())
        .numbers
        .counts(base, head)
}

#[test]
fn a_ref_moved_between_the_two_reads_stores_nothing_under_the_old_commit() {
    let rig = RepoRig::new("bl-moved");
    let int = git_out(&rig.repo, &["rev-parse", "int"]);
    let old = git_out(&rig.repo, &["rev-parse", "lane-a"]);
    let repo = rig.repo.clone();
    let target = int.clone();
    rig.launcher.before_once("ahead-behind", move || {
        git(&repo, &["update-ref", "refs/heads/lane-a", &target]);
    });
    let key = key_of(&rig);
    reply(&rig);
    idle(&rig, &key);
    assert_eq!(
        counts_of(&rig, &key, &int, &old),
        Some(Some(Counts {
            ahead: 2,
            behind: 0
        }))
    );
}

#[test]
fn a_failing_lines_read_is_remembered_and_the_row_settles_without_numbers() {
    let rig = RepoRig::new("bl-lines-fail");
    rig.launcher.fail("diff", Some("--shortstat"));
    let key = key_of(&rig);
    let first = reply(&rig);
    idle(&rig, &key);
    assert_eq!(row(&first, "lane-a")["pending"], false, "{first}");
    let before = shortstats(&rig);
    assert!(before >= 1);
    let second = reply(&rig);
    idle(&rig, &key);
    assert_eq!(shortstats(&rig), before, "the failed diff ran again");
    let lane = row(&second, "lane-a");
    assert_eq!(lane["pending"], false, "{second}");
    assert_eq!(lane["lines"], Value::Null);
    assert_eq!(second["pending"], 0, "{second}");
}

#[test]
fn a_failing_count_read_is_remembered_and_the_row_settles_without_numbers() {
    let rig = RepoRig::new("bl-counts-fail");
    rig.launcher.old_git(true);
    rig.launcher.fail("rev-list", Some("--left-right"));
    let key = key_of(&rig);
    let first = reply(&rig);
    idle(&rig, &key);
    assert_eq!(row(&first, "lane-a")["pending"], false, "{first}");
    rig.launcher.clear();
    let second = reply(&rig);
    idle(&rig, &key);
    assert_eq!(
        rig.launcher.count("rev-list"),
        0,
        "the failed count ran again"
    );
    assert_eq!(row(&second, "lane-a")["pending"], false, "{second}");
    assert_eq!(row(&second, "lane-a")["ahead"], Value::Null);
    assert_eq!(second["pending"], 0, "{second}");
}
