use crate::acting_ops::now_ms;
use crate::git_ops::{lock, Folders, GitOps, GIT_OPS};
use crate::git_testkit::{git, git_out, Recorded, RepoRig};
use crate::inbox_ops_tests::app;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use notchling_engine::git::reader::Reader;
use notchling_local_api::{ops, ApiError, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn ask(rig: &RepoRig, cwd: &str, extra: Value) -> Result<Value, ApiError> {
    let mut args = json!({ "cwd": cwd });
    if let Some(map) = extra.as_object() {
        for (key, value) in map {
            args[key] = value.clone();
        }
    }
    rig.ops.call(&app(), ops::GIT_WORKTREES, &args)
}

fn reply(rig: &RepoRig) -> Value {
    ask(rig, &rig.cwd(), json!({})).unwrap()
}

fn entry<'a>(reply: &'a Value, suffix: &str) -> &'a Value {
    reply["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["path"].as_str().unwrap().ends_with(suffix))
        .unwrap_or_else(|| panic!("no worktree {suffix} in {reply}"))
}

fn key_of(rig: &RepoRig) -> PathBuf {
    let reader = Reader::open(&rig.cwd(), &*rig.launcher).unwrap();
    let key = reader.common_dir().unwrap();
    rig.launcher.clear();
    key
}

fn idle(rig: &RepoRig, key: &Path) {
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

fn all_read(reply: &Value) -> bool {
    reply["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["current"] == false && w["missing"] == false && !w["base"].is_null())
        .all(|w| !w["work"].is_null())
}

fn read_all(rig: &RepoRig, cwd: &str) -> Value {
    let key = key_of(rig);
    for _ in 0..8 {
        let got = ask(rig, cwd, json!({})).unwrap();
        idle(rig, &key);
        if all_read(&got) {
            return ask(rig, cwd, json!({})).unwrap();
        }
    }
    panic!("work never read");
}

fn placed(rig: &RepoRig) -> Value {
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        let got = reply(rig);
        let pending = got["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["missing"] == false && w["base"].is_null());
        if !pending || Instant::now() >= end {
            return got;
        }
    }
}

fn settled(rig: &RepoRig) -> Value {
    let got = read_all(rig, &rig.cwd());
    idle(rig, &key_of(rig));
    rig.launcher.clear();
    got
}

#[test]
fn the_op_is_app_quiet_and_in_git_ops() {
    let rig = RepoRig::new("wt-spec");
    assert_eq!(rig.ops.spec(ops::GIT_WORKTREES), Some(OpSpec::APP_QUIET));
    assert!(GIT_OPS.contains(&ops::GIT_WORKTREES));
}

#[test]
fn cwd_not_a_session_folder_is_refused() {
    let rig = RepoRig::new("wt-refuse");
    let other = TempDir::new("wt-refuse-other");
    other.write("x/a.txt", "x");
    let cwd = other.path("x").to_string_lossy().into_owned();
    let err = ask(&rig, &cwd, json!({})).unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(rig.launcher.seen().is_empty());
}

#[test]
fn not_a_repository_is_repo_false() {
    let t = TempDir::new("wt-norepo");
    t.write("plain/a.txt", "x");
    let known = vec![t.root().to_path_buf()];
    let folders: Folders = Arc::new(move || known.clone());
    let ops_ = GitOps::new(Recorded::new(), folders);
    let cwd = t.path("plain").to_string_lossy().into_owned();
    let got = ops_
        .call(&app(), ops::GIT_WORKTREES, &json!({"cwd": cwd}))
        .unwrap();
    assert_eq!(got, json!({"repo": false}));
}

#[test]
fn lists_main_and_linked_with_branch_head_and_base() {
    let rig = RepoRig::new("wt-list");
    let got = reply(&rig);
    assert_eq!(got["repo"], true);
    assert_eq!(got["name"], "repo");
    assert_eq!(got["default"], "main");
    assert_eq!(got["integration"], "int");
    assert_eq!(got["truncated"], false);
    assert!(got["fetched_at"].as_i64().is_some());
    assert_eq!(got["worktrees"].as_array().unwrap().len(), 3);
    let main = entry(&got, "/repo");
    let real = std::fs::canonicalize(&rig.repo).unwrap();
    assert_eq!(main["real"], real.to_string_lossy().as_ref());
    assert_eq!(
        (main["main"].clone(), main["current"].clone()),
        (json!(true), json!(true))
    );
    assert_eq!(main["branch"], "int");
    let int = git_out(&rig.repo, &["rev-parse", "int"]);
    assert!(int.starts_with(main["head"].as_str().unwrap()));
    assert_eq!(main["head"].as_str().unwrap().len(), 7);
    assert_eq!(main["base"], "main");
    assert_eq!(
        (main["ahead"].clone(), main["behind"].clone()),
        (json!(3), json!(0))
    );
    assert_eq!(main["unrelated"], false);
    assert_eq!(main["upstream"], Value::Null);
    assert_eq!(main["missing"], false);
    let lane = entry(&got, "lane-a");
    assert_eq!(lane["branch"], "lane-a");
    assert_eq!(
        (lane["main"].clone(), lane["current"].clone()),
        (json!(false), json!(false))
    );
    assert!(lane["committed_at"].as_i64().is_some());
}

#[test]
fn lane_base_is_the_integration_branch() {
    let rig = RepoRig::new("wt-base");
    let got = reply(&rig);
    let lane = entry(&got, "lane-a");
    assert_eq!(lane["base"], "int");
    assert_eq!(
        (lane["ahead"].clone(), lane["behind"].clone()),
        (json!(2), json!(0))
    );
    let detached = entry(&got, "lane-b");
    assert_eq!(detached["base"], "int");
    assert_eq!(detached["ahead"], 0);
}

#[test]
fn current_is_the_longest_path() {
    let rig = RepoRig::new("wt-current");
    let nested = rig.lane("repo/nested", &["-b", "nested"], &["int"]);
    let at_nested = ask(&rig, &nested.to_string_lossy(), json!({})).unwrap();
    let flags = |reply: &Value, suffix: &str| entry(reply, suffix)["current"].clone();
    assert_eq!(flags(&at_nested, "/nested"), json!(true));
    assert_eq!(flags(&at_nested, "/repo"), json!(false));
    assert_eq!(flags(&at_nested, "lane-a"), json!(false));
    let at_main = reply(&rig);
    assert_eq!(flags(&at_main, "/repo"), json!(true));
    assert_eq!(flags(&at_main, "/nested"), json!(false));
}

#[test]
fn current_work_is_null() {
    let rig = RepoRig::new("wt-work-null");
    let cwd = rig.lane_a.to_string_lossy().into_owned();
    let got = read_all(&rig, &cwd);
    assert_eq!(entry(&got, "lane-a")["work"], Value::Null);
    let main = entry(&got, "/repo");
    assert_eq!(main["work"]["files"], 3);
    assert!(main["work"]["read_at"].as_i64().is_some());
    let at_main = read_all(&rig, &rig.cwd());
    assert_eq!(entry(&at_main, "/repo")["work"], Value::Null);
    assert_eq!(entry(&at_main, "lane-a")["work"]["uncommitted_files"], 1);
}

#[test]
fn missing_work_read_first_up_to_four() {
    let rig = RepoRig::new("wt-four");
    for n in 0..4 {
        rig.lane(&format!("extra-{n}"), &["--detach"], &["int"]);
    }
    let key = key_of(&rig);
    let mut total = 0;
    let mut got = Value::Null;
    for _ in 0..8 {
        rig.launcher.clear();
        got = reply(&rig);
        idle(&rig, &key);
        let reads = rig.launcher.count("status");
        assert!(reads <= 4, "{reads} reads in one call");
        total += reads;
        if all_read(&got) {
            break;
        }
    }
    let again = reply(&rig);
    assert!(all_read(&again));
    assert_eq!(total, 6);
    assert!(got["worktrees"].as_array().unwrap().len() == 7);
}

#[test]
fn second_call_inside_15s_runs_no_status() {
    let rig = RepoRig::new("wt-cached");
    let first = settled(&rig);
    assert!(!entry(&first, "lane-a")["work"].is_null());
    let second = reply(&rig);
    assert_eq!(rig.launcher.count("status"), 0);
    assert_eq!(entry(&second, "lane-a")["work"]["files"], 3);
    assert_eq!(rig.launcher.count("worktree"), 0);
}

#[test]
fn fresh_path_is_read_again() {
    let rig = RepoRig::new("wt-fresh");
    settled(&rig);
    let key = key_of(&rig);
    let fresh = json!({"fresh": [rig.lane_a.to_string_lossy()]});
    ask(&rig, &rig.cwd(), fresh).unwrap();
    idle(&rig, &key);
    assert_eq!(rig.launcher.count("status"), 1);
    assert_eq!(rig.launcher.count_at("status", "lane-a"), 1);
}

#[test]
fn unknown_fresh_ignored() {
    let rig = RepoRig::new("wt-fresh-odd");
    settled(&rig);
    let key = key_of(&rig);
    rig.t.write("elsewhere/x.txt", "x");
    let odd =
        json!({"fresh": [rig.t.path("elsewhere").to_string_lossy(), "/etc", "relative", 7, null]});
    let got = ask(&rig, &rig.cwd(), odd).unwrap();
    idle(&rig, &key);
    assert_eq!(got["repo"], true);
    assert_eq!(rig.launcher.count("status"), 0);
}

#[test]
fn detached_worktree_row() {
    let rig = RepoRig::new("wt-detached");
    let got = reply(&rig);
    let row = entry(&got, "lane-b");
    assert_eq!(row["detached"], true);
    assert_eq!(row["branch"], Value::Null);
    assert_eq!(row["head"].as_str().unwrap().len(), 7);
    assert_eq!(row["base"], "int");
    assert_eq!(
        (row["ahead"].clone(), row["behind"].clone()),
        (json!(0), json!(0))
    );
    assert!(row["committed_at"].as_i64().is_some());
    assert_eq!(row["upstream"], Value::Null);
}

#[test]
fn prunable_folder_is_missing_without_numbers() {
    let rig = RepoRig::new("wt-prunable");
    std::fs::remove_dir_all(&rig.lane_a).unwrap();
    let key = key_of(&rig);
    let got = reply(&rig);
    idle(&rig, &key);
    let row = entry(&got, "lane-a");
    assert_eq!(row["missing"], true);
    assert_eq!(row["base"], Value::Null);
    assert_eq!(row["ahead"], Value::Null);
    assert_eq!(row["work"], Value::Null);
    assert_eq!(row["unrelated"], false);
    assert_eq!(rig.launcher.count_at("status", "lane-a"), 0);
}

#[test]
fn a_locked_worktree_whose_folder_is_gone_is_missing_and_never_read() {
    let rig = RepoRig::new("wt-locked-gone");
    let dir = rig.lane("locked-gone", &["-b", "lk"], &["int"]);
    git(&rig.repo, &["worktree", "lock", dir.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).unwrap();
    let key = key_of(&rig);
    let got = reply(&rig);
    idle(&rig, &key);
    let row = entry(&got, "locked-gone");
    assert_eq!(row["locked"], true, "{row}");
    assert_eq!(row["missing"], true, "{row}");
    assert_eq!(row["base"], Value::Null);
    assert_eq!(row["work"], Value::Null);
    let again = reply(&rig);
    idle(&rig, &key);
    assert_eq!(entry(&again, "locked-gone")["missing"], true);
    assert_eq!(rig.launcher.count_at("status", "locked-gone"), 0);
    assert_eq!(rig.launcher.count_at("diff", "locked-gone"), 0);
}

#[test]
fn bare_main_left_out_without_integration() {
    let rig = RepoRig::new("wt-bare");
    let bare = rig.t.path("bare.git");
    git(
        rig.t.root(),
        &["clone", "-q", "--bare", rig.cwd().as_str(), "bare.git"],
    );
    let w1 = rig.t.path("w1");
    let w2 = rig.t.path("w2");
    git(
        &bare,
        &["worktree", "add", "-q", w1.to_str().unwrap(), "int"],
    );
    git(
        &bare,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            w2.to_str().unwrap(),
            "main",
        ],
    );
    let got = ask(&rig, &w1.to_string_lossy(), json!({})).unwrap();
    assert_eq!(got["repo"], true);
    assert_eq!(got["name"], "bare");
    assert_eq!(got["integration"], Value::Null);
    assert_eq!(got["default"], "main");
    let list = got["worktrees"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert!(list.iter().all(|w| w["main"] == false));
    let row = entry(&got, "w1");
    assert_eq!(row["current"], true);
    assert_eq!(row["base"], "main");
    assert_eq!(row["ahead"], 3);
    assert_eq!(entry(&got, "w2")["detached"], true);
}

#[test]
fn unrelated_lane_has_no_base_and_no_work_read() {
    let rig = RepoRig::new("wt-unrelated");
    rig.lane("lane-o", &[], &["orphan"]);
    let key = key_of(&rig);
    let got = reply(&rig);
    idle(&rig, &key);
    let row = entry(&got, "lane-o");
    assert_eq!(row["base"], Value::Null);
    assert_eq!(row["unrelated"], true);
    assert_eq!(row["work"], Value::Null);
    assert_eq!(rig.launcher.count_at("status", "lane-o"), 0);
    assert!(rig.launcher.count("merge-base") >= 1);
    let again = reply(&rig);
    assert_eq!(entry(&again, "lane-o")["unrelated"], true);
    assert_eq!(rig.launcher.count_at("status", "lane-o"), 0);
}

#[test]
fn upstream_candidate_counted_by_rev_list() {
    let rig = RepoRig::new("wt-upstream");
    git(
        &rig.repo,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );
    git(
        &rig.repo,
        &["update-ref", "refs/remotes/origin/int", "lane-a"],
    );
    let lane = rig.lane("lane-u", &["-b", "lane-u"], &["origin/int"]);
    rig.t.write("lane-u/u1.txt", "u\n");
    git(&lane, &["add", "."]);
    git(&lane, &["commit", "-q", "-m", "lane u"]);
    let got = reply(&rig);
    let row = entry(&got, "lane-u");
    assert_eq!(row["upstream"], "origin/int");
    assert_eq!(row["upstream_ahead"], 1);
    assert_eq!(row["base"], "origin/int");
    assert_eq!(
        (row["ahead"].clone(), row["behind"].clone()),
        (json!(1), json!(0))
    );
    assert!(rig.launcher.ran("rev-list", "--left-right"));
}

#[test]
fn no_worktree_counts_after_dedup() {
    let rig = RepoRig::new("wt-count");
    git(
        &rig.repo,
        &["update-ref", "refs/remotes/origin/merged", "merged"],
    );
    let got = reply(&rig);
    assert_eq!(got["no_worktree"], 3);
}

#[test]
fn fetched_at_from_fetch_head() {
    let rig = RepoRig::new("wt-fetched");
    let got = reply(&rig);
    let meta = std::fs::metadata(rig.repo.join(".git/FETCH_HEAD")).unwrap();
    let stamp = meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert_eq!(got["fetched_at"], stamp);
    std::fs::remove_file(rig.repo.join(".git/FETCH_HEAD")).unwrap();
    let fresh = RepoRig::new("wt-fetched-none");
    std::fs::remove_file(fresh.repo.join(".git/FETCH_HEAD")).unwrap();
    assert_eq!(reply(&fresh)["fetched_at"], Value::Null);
}

#[test]
fn old_git_falls_back_to_rev_list() {
    let rig = RepoRig::new("wt-old-git");
    rig.launcher.old_git(true);
    let got = placed(&rig);
    let lane = entry(&got, "lane-a");
    assert_eq!(lane["base"], "int");
    assert_eq!(
        (lane["ahead"].clone(), lane["behind"].clone()),
        (json!(2), json!(0))
    );
    assert!(rig.launcher.ran("for-each-ref", "ahead-behind"));
    assert!(rig.launcher.ran("rev-list", "--left-right"));
    assert_eq!(entry(&got, "/repo")["base"], "main");
}

#[test]
fn no_read_starts_after_one_and_a_half_seconds() {
    let rig = RepoRig::new("wt-budget");
    rig.lane("lane-c", &["--detach"], &["int"]);
    let key = key_of(&rig);
    rig.launcher
        .delay("status", Some(Duration::from_millis(800)));
    let started = Instant::now();
    let got = reply(&rig);
    assert!(started.elapsed() < Duration::from_millis(3_500));
    idle(&rig, &key);
    let reads = rig.launcher.count("status");
    assert!((1..=2).contains(&reads), "{reads} reads");
    for seen in rig.launcher.seen().iter().filter(|s| s.sub == "status") {
        assert!(seen.at.duration_since(started) < Duration::from_millis(1_600));
    }
    let read = got["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| !w["work"].is_null())
        .count();
    assert!(read <= 2);
}

#[test]
fn timed_out_reads_fill_the_cache() {
    let rig = RepoRig::new("wt-late");
    rig.lane("lane-c", &["--detach"], &["int"]);
    let key = key_of(&rig);
    rig.launcher
        .delay("status", Some(Duration::from_millis(800)));
    let first = reply(&rig);
    let done_in_call = first["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| !w["work"].is_null())
        .count();
    idle(&rig, &key);
    let started = rig.launcher.count("status");
    assert!(done_in_call < started || started == 1);
    rig.launcher.delay("status", None);
    rig.launcher.clear();
    let second = read_all(&rig, &rig.cwd());
    assert!(all_read(&second));
    assert_eq!(rig.launcher.count("status"), 3 - started);
}

#[test]
fn cache_lock_never_held_across_git() {
    let rig = Arc::new(RepoRig::new("wt-lock"));
    let key = key_of(&rig);
    rig.launcher
        .delay("status", Some(Duration::from_millis(2_000)));
    let done = Arc::new(AtomicBool::new(false));
    let worker = {
        let (rig, done) = (rig.clone(), done.clone());
        std::thread::spawn(move || {
            let got = reply(&rig);
            done.store(true, Ordering::SeqCst);
            got
        })
    };
    let end = Instant::now() + Duration::from_secs(20);
    while rig.launcher.count("status") == 0 {
        assert!(Instant::now() < end, "no read started");
        std::thread::sleep(Duration::from_millis(10));
    }
    let second = reply(&rig);
    assert_eq!(second["repo"], true);
    assert!(
        !done.load(Ordering::SeqCst),
        "the second call waited for the first"
    );
    worker.join().unwrap();
    idle(&rig, &key);
}
