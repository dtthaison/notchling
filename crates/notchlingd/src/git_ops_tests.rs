use crate::git_ops::{live_folders, Folders, GitOps};
use crate::inbox_ops_tests::app;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use notchling_engine::control::launcher::{FakeLauncher, LaunchOutput};
use notchling_engine::search::index::TitledSession;
use notchling_local_api::{ops, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn z(records: &[&str]) -> String {
    let mut text = records.join("\0");
    text.push('\0');
    text
}

fn folders_of(list: Vec<PathBuf>) -> Folders {
    Arc::new(move || list.clone())
}

fn rig(known: Vec<PathBuf>) -> (GitOps, Arc<FakeLauncher>) {
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::failed(1, "")));
    (GitOps::new(fake.clone(), folders_of(known)), fake)
}

fn call(ops: &GitOps, op: &str, args: Value) -> Result<Value, notchling_local_api::ApiError> {
    ops.call(&app(), op, &args)
}

#[test]
fn both_ops_are_app_tier_reads_and_nothing_else_is_served() {
    let (git, _) = rig(vec![]);
    assert_eq!(git.spec(ops::GIT_STATUS), Some(OpSpec::APP_QUIET));
    assert_eq!(git.spec(ops::GIT_DIFF), Some(OpSpec::APP_QUIET));
    assert_ne!(git.spec(ops::GIT_STATUS), Some(OpSpec::APP));
    assert_eq!(git.spec(ops::SESSIONS_LIST), None);
    assert_eq!(OpSpec::APP_QUIET.tier, Tier::App);
}

#[test]
fn status_of_a_session_folder_answers_the_branch_and_the_files() {
    let t = TempDir::new("gitops-status");
    t.write("proj/a.rs", "x");
    let (git, fake) = rig(vec![t.path("proj")]);
    fake.push_output(Ok(LaunchOutput::ok(&z(&[
        "# branch.oid abc1234def",
        "# branch.head main",
        "# branch.upstream origin/main",
        "# branch.ab +1 -0",
        "1 .M N... 100644 100644 100644 a b a.rs",
    ]))));
    fake.push_output(Ok(LaunchOutput::ok(&z(&["2\t1\ta.rs"]))));
    let cwd = t.path("proj").to_string_lossy().into_owned();
    let got = call(&git, ops::GIT_STATUS, json!({"cwd": cwd})).unwrap();
    let real = std::fs::canonicalize(t.path("proj")).unwrap();
    for argv in fake.commands().iter().map(|c| &c.argv) {
        let at = argv.iter().position(|a| a == "-C").unwrap();
        assert_eq!(
            argv[at + 1],
            real.to_string_lossy(),
            "runs where it checked"
        );
    }
    assert_eq!(got["repo"], true);
    assert_eq!(got["branch"], "main");
    assert_eq!(got["upstream"], "origin/main");
    assert_eq!(got["ahead"], 1);
    assert_eq!(got["changes"][0]["path"], "a.rs");
    assert_eq!(got["changes"][0]["status"], "M");
    assert_eq!(got["changes"][0]["added"], 2);
    assert_eq!(got["changes"][0]["removed"], 1);
    assert_eq!(got["truncated"], false);
}

#[test]
fn a_folder_that_is_not_a_repository_says_so_in_one_reply() {
    let t = TempDir::new("gitops-norepo");
    t.write("proj/a.rs", "x");
    let (git, fake) = rig(vec![t.path("proj")]);
    fake.push_output(Ok(LaunchOutput {
        code: 128,
        output: String::new(),
        stderr: "fatal: not a git repository\n".into(),
    }));
    fake.push_output(Ok(LaunchOutput::failed(128, "")));
    let cwd = t.path("proj").to_string_lossy().into_owned();
    let got = call(&git, ops::GIT_STATUS, json!({"cwd": cwd})).unwrap();
    assert_eq!(got, json!({"repo": false}));
}

#[test]
fn a_folder_below_a_session_folder_is_served_and_others_are_forbidden_without_running_anything() {
    let t = TempDir::new("gitops-known");
    t.write("proj/sub/a.rs", "x");
    t.write("other/b.rs", "x");
    let (git, fake) = rig(vec![t.path("proj")]);
    fake.push_output(Ok(LaunchOutput::ok(&z(&["# branch.head main"]))));
    let below = t.path("proj/sub").to_string_lossy().into_owned();
    assert!(call(&git, ops::GIT_STATUS, json!({"cwd": below})).is_ok());
    let before = fake.commands().len();
    let bad = [
        t.path("other").to_string_lossy().into_owned(),
        t.path("proj/../other").to_string_lossy().into_owned(),
        t.path("proj/missing").to_string_lossy().into_owned(),
        "relative/path".to_owned(),
        "/".to_owned(),
    ];
    for cwd in bad {
        for op in [ops::GIT_STATUS, ops::GIT_DIFF] {
            let err = call(&git, op, json!({"cwd": cwd, "path": "a.rs"})).unwrap_err();
            assert_eq!(err.code, ErrorCode::Forbidden, "{cwd} {op}");
        }
    }
    assert_eq!(fake.commands().len(), before);
}

#[test]
fn a_missing_cwd_or_path_is_a_bad_request() {
    let t = TempDir::new("gitops-args");
    t.write("proj/a.rs", "x");
    let (git, _) = rig(vec![t.path("proj")]);
    let cwd = t.path("proj").to_string_lossy().into_owned();
    for (op, args) in [
        (ops::GIT_STATUS, json!({})),
        (ops::GIT_STATUS, json!({"cwd": ""})),
        (ops::GIT_STATUS, json!({"cwd": 7})),
        (ops::GIT_DIFF, json!({"cwd": cwd})),
    ] {
        let err = call(&git, op, args).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest, "{op}");
    }
}

#[test]
fn a_diff_path_that_escapes_the_repository_is_forbidden() {
    let t = TempDir::new("gitops-escape");
    t.write("proj/a.rs", "x");
    t.write("secret.txt", "s");
    let (git, fake) = rig(vec![t.path("proj")]);
    let cwd = t.path("proj").to_string_lossy().into_owned();
    for path in ["../secret.txt", "/etc/passwd", "a/../../secret.txt"] {
        let err = call(&git, ops::GIT_DIFF, json!({"cwd": cwd, "path": path})).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden, "{path}");
    }
    assert!(fake.commands().is_empty());
}

#[test]
fn a_refused_folder_does_not_rescan_the_known_ones_until_they_are_stale() {
    let t = TempDir::new("gitops-rescan");
    t.write("proj/a.rs", "x");
    t.write("other/b.rs", "x");
    let scans = Arc::new(Mutex::new(0usize));
    let counted: Folders = {
        let (scans, known) = (scans.clone(), vec![t.path("proj")]);
        Arc::new(move || {
            *scans.lock().unwrap() += 1;
            known.clone()
        })
    };
    let git = GitOps::new(Arc::new(FakeLauncher::new()), counted);
    let other = t.path("other").to_string_lossy().into_owned();
    for _ in 0..5 {
        assert!(call(&git, ops::GIT_STATUS, json!({"cwd": other})).is_err());
    }
    let mine = t.path("proj").to_string_lossy().into_owned();
    assert!(call(&git, ops::GIT_STATUS, json!({"cwd": mine})).is_ok());
    assert_eq!(*scans.lock().unwrap(), 1);
}

#[cfg(unix)]
#[test]
fn a_diff_path_through_a_link_out_is_forbidden() {
    let t = TempDir::new("gitops-link");
    t.write("proj/a.rs", "x");
    t.write("outside/secret.txt", "s");
    std::os::unix::fs::symlink(t.path("outside"), t.path("proj/out")).unwrap();
    let (git, fake) = rig(vec![t.path("proj")]);
    let root = t.path("proj").to_string_lossy().into_owned();
    let real = std::fs::canonicalize(t.path("proj")).unwrap();
    fake.push_output(Ok(LaunchOutput::ok(&real.to_string_lossy())));
    let err = call(
        &git,
        ops::GIT_DIFF,
        json!({"cwd": root, "path": "out/secret.txt"}),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
}

#[test]
fn a_diff_answers_its_text_and_whether_it_was_cut() {
    let t = TempDir::new("gitops-diff");
    t.write("proj/a.rs", "x");
    let root = t.path("proj").to_string_lossy().into_owned();
    let (git, fake) = rig(vec![t.path("proj")]);
    fake.push_output(Ok(LaunchOutput::ok(&root)));
    fake.push_output(Ok(LaunchOutput::ok(&z(&[
        "# branch.head main",
        "1 .M N... 100644 100644 100644 a b a.rs",
    ]))));
    fake.push_output(Ok(LaunchOutput::ok("diff --git a/a.rs b/a.rs\n-x\n+y\n")));
    let got = call(&git, ops::GIT_DIFF, json!({"cwd": root, "path": "a.rs"})).unwrap();
    assert_eq!(got["path"], "a.rs");
    assert_eq!(got["text"], "diff --git a/a.rs b/a.rs\n-x\n+y\n");
    assert_eq!(got["truncated"], false);
    assert_eq!(got["untracked"], false);
}

#[test]
fn the_known_folders_are_the_sessions_antigravity_rows_and_codex_rollouts() {
    let t = TempDir::new("gitops-folders");
    let sessions = Arc::new(Mutex::new(json!([
        {"id": "a", "cwd": "/w/claude"},
        {"id": "b", "cwd": ""},
        {"id": "c"},
    ])));
    let titles: crate::pages_ops::Titles = Arc::new(|| {
        vec![
            TitledSession {
                id: "g".into(),
                title: "t".into(),
                folder: "/w/agy".into(),
                modified: 0.0,
            },
            TitledSession {
                id: "h".into(),
                title: "t".into(),
                folder: String::new(),
                modified: 0.0,
            },
        ]
    });
    t.write(
        "codex/2026/11/13/rollout-2026-11-13T10-00-00-c1.jsonl",
        concat!(
            r#"{"timestamp":"2026-11-13T10:00:00.000Z","type":"session_meta","payload":{"id":"c1","cwd":"/w/codex"}}"#,
            "\n"
        ),
    );
    let list = live_folders(sessions, titles, t.path("codex"), true)();
    assert_eq!(
        list,
        [
            PathBuf::from("/w/agy"),
            PathBuf::from("/w/claude"),
            PathBuf::from("/w/codex")
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_real_repository_in_a_session_folder_reads_through_the_ops() {
    use notchling_engine::control::RealLauncher;
    let t = TempDir::new("gitops-real");
    let proj = t.path("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let git_in = |args: &[&str]| {
        std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(["-c", "commit.gpgsign=false", "-C"])
            .arg(&proj)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    assert!(git_in(&["init", "-q", "-b", "main"]), "git init failed");
    t.write("proj/a.txt", "one\n");
    assert!(git_in(&["add", "."]));
    assert!(git_in(&["commit", "-q", "-m", "first"]));
    t.write("proj/a.txt", "one\ntwo\n");
    t.write("proj/new.txt", "fresh\n");
    let ops_ = GitOps::new(
        Arc::new(RealLauncher::with_claude(PathBuf::from("/nonexistent"))),
        folders_of(vec![proj.clone()]),
    );
    let cwd = proj.to_string_lossy().into_owned();
    let status = call(&ops_, ops::GIT_STATUS, json!({"cwd": cwd})).unwrap();
    assert_eq!(status["branch"], "main");
    assert_eq!(status["changes"].as_array().unwrap().len(), 2);
    assert_eq!(status["changes"][0]["added"], 1);
    let diff = call(&ops_, ops::GIT_DIFF, json!({"cwd": cwd, "path": "a.txt"})).unwrap();
    assert!(diff["text"].as_str().unwrap().contains("+two"));
    let fresh = call(&ops_, ops::GIT_DIFF, json!({"cwd": cwd, "path": "new.txt"})).unwrap();
    assert_eq!(fresh["untracked"], true);
    assert!(fresh["text"].as_str().unwrap().contains("+fresh"));
}
