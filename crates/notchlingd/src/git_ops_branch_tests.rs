use crate::git_ops::{Folders, GitOps, GIT_OPS};
use crate::inbox_ops_tests::app;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
#[cfg(unix)]
use notchling_engine::control::launcher::Launcher;
use notchling_engine::control::launcher::{FakeLauncher, LaunchError, LaunchOutput};
#[cfg(target_os = "macos")]
use notchling_engine::git::SandboxedLauncher;
use notchling_local_api::{ops, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::Duration;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const BRANCH_OPS: [&str; 3] = [
    ops::GIT_BRANCH_DIFF_STAT,
    ops::GIT_BRANCH_DIFF_FILE,
    ops::GIT_BRANCHES,
];

fn z(records: &[&str]) -> String {
    let mut text = records.join("\0");
    text.push('\0');
    text
}

fn rig(known: Vec<PathBuf>) -> (GitOps, Arc<FakeLauncher>) {
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::failed(1, "")));
    let folders: Folders = Arc::new(move || known.clone());
    (GitOps::new(fake.clone(), folders), fake)
}

fn call(ops: &GitOps, op: &str, args: Value) -> Result<Value, notchling_local_api::ApiError> {
    ops.call(&app(), op, &args)
}

#[test]
fn the_branch_ops_are_app_tier_quiet_reads_and_part_of_the_group() {
    let (git, _) = rig(vec![]);
    for op in BRANCH_OPS {
        assert_eq!(git.spec(op), Some(OpSpec::APP_QUIET), "{op}");
        assert!(GIT_OPS.contains(&op), "{op}");
    }
    assert_eq!(OpSpec::APP_QUIET.tier, Tier::App);
    assert_eq!(GIT_OPS.len(), 5);
}

#[test]
fn a_stat_answers_the_branch_the_base_the_files_and_the_totals() {
    let t = TempDir::new("gitops-stat");
    t.write("proj/a.rs", "x");
    let (git, fake) = rig(vec![t.path("proj")]);
    fake.push_output(Ok(LaunchOutput::ok(&z(&[
        "# branch.oid abc1234def",
        "# branch.head feature",
        "1 .M N... 100644 100644 100644 a b a.rs",
    ]))));
    fake.push_output(Ok(LaunchOutput::failed(1, "")));
    fake.push_output(Ok(LaunchOutput::ok(SHA)));
    fake.push_output(Ok(LaunchOutput::ok(&z(&["M", "a.rs", "A", "n.rs"]))));
    fake.push_output(Ok(LaunchOutput::ok(&z(&["3\t1\ta.rs", "5\t0\tn.rs"]))));
    let cwd = t.path("proj").to_string_lossy().into_owned();
    let got = call(&git, ops::GIT_BRANCH_DIFF_STAT, json!({"cwd": cwd})).unwrap();
    let real = std::fs::canonicalize(t.path("proj")).unwrap();
    for argv in fake.commands().iter().map(|c| &c.argv) {
        let at = argv.iter().position(|a| a == "-C").unwrap();
        assert_eq!(argv[at + 1], real.to_string_lossy());
    }
    assert_eq!(got["repo"], true);
    assert_eq!(got["includes_uncommitted"], true);
    assert_eq!(got["branch"], "feature");
    assert_eq!(got["base"], "main");
    assert_eq!(got["merge_base"], "0123456");
    assert_eq!(got["added"], 8);
    assert_eq!(got["removed"], 1);
    assert_eq!(got["file_count"], 2);
    assert_eq!(got["uncommitted_files"], 1);
    assert_eq!(got["truncated"], false);
    assert_eq!(got["files"][0]["path"], "a.rs");
    assert_eq!(got["files"][0]["status"], "M");
    assert_eq!(got["files"][0]["uncommitted"], true);
    assert_eq!(got["files"][1]["status"], "A");
    assert_eq!(got["files"][1]["from"], Value::Null);
}

#[test]
fn a_folder_that_is_not_a_repository_answers_repo_false_for_the_listing_ops() {
    let t = TempDir::new("gitops-branch-norepo");
    t.write("proj/a.rs", "x");
    let cwd = t.path("proj").to_string_lossy().into_owned();
    for op in [ops::GIT_BRANCH_DIFF_STAT, ops::GIT_BRANCHES] {
        let (git, fake) = rig(vec![t.path("proj")]);
        fake.push_output(Ok(LaunchOutput {
            code: 128,
            output: String::new(),
            stderr: "fatal: not a git repository\n".into(),
        }));
        fake.push_output(Ok(LaunchOutput::failed(128, "")));
        assert_eq!(
            call(&git, op, json!({"cwd": cwd})).unwrap(),
            json!({"repo": false}),
            "{op}"
        );
    }
}

#[test]
fn every_branch_op_refuses_a_folder_that_is_not_a_session_folder_without_running_anything() {
    let t = TempDir::new("gitops-branch-known");
    t.write("proj/a.rs", "x");
    t.write("other/b.rs", "x");
    let (git, fake) = rig(vec![t.path("proj")]);
    let bad = [
        t.path("other").to_string_lossy().into_owned(),
        t.path("proj/../other").to_string_lossy().into_owned(),
        t.path("proj/missing").to_string_lossy().into_owned(),
        "relative/path".to_owned(),
        "/".to_owned(),
    ];
    for cwd in bad {
        for op in BRANCH_OPS {
            let args = json!({"cwd": cwd, "base": "main", "path": "a.rs"});
            let err = call(&git, op, args).unwrap_err();
            assert_eq!(err.code, ErrorCode::Forbidden, "{cwd} {op}");
        }
    }
    assert!(fake.commands().is_empty());
}

#[test]
fn a_missing_cwd_base_or_path_is_a_bad_request() {
    let t = TempDir::new("gitops-branch-args");
    t.write("proj/a.rs", "x");
    let (git, fake) = rig(vec![t.path("proj")]);
    let cwd = t.path("proj").to_string_lossy().into_owned();
    for (op, args) in [
        (ops::GIT_BRANCH_DIFF_STAT, json!({})),
        (ops::GIT_BRANCHES, json!({"cwd": ""})),
        (
            ops::GIT_BRANCH_DIFF_FILE,
            json!({"cwd": cwd, "path": "a.rs"}),
        ),
        (
            ops::GIT_BRANCH_DIFF_FILE,
            json!({"cwd": cwd, "base": "", "path": "a.rs"}),
        ),
        (
            ops::GIT_BRANCH_DIFF_FILE,
            json!({"cwd": cwd, "base": "main"}),
        ),
        (
            ops::GIT_BRANCH_DIFF_FILE,
            json!({"cwd": cwd, "base": 3, "path": "a.rs"}),
        ),
    ] {
        let err = call(&git, op, args.clone()).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest, "{op} {args}");
    }
    assert!(fake.commands().is_empty());
}

#[test]
fn an_odd_base_or_an_escaping_path_is_forbidden_before_git_runs() {
    let t = TempDir::new("gitops-branch-odd");
    t.write("proj/a.rs", "x");
    let (git, fake) = rig(vec![t.path("proj")]);
    let cwd = t.path("proj").to_string_lossy().into_owned();
    for base in ["--output=/tmp/x", "a b", "-x", "a..b"] {
        for (op, args) in [
            (ops::GIT_BRANCH_DIFF_STAT, json!({"cwd": cwd, "base": base})),
            (
                ops::GIT_BRANCH_DIFF_FILE,
                json!({"cwd": cwd, "base": base, "path": "a.rs"}),
            ),
        ] {
            let err = call(&git, op, args).unwrap_err();
            assert_eq!(err.code, ErrorCode::Forbidden, "{base} {op}");
        }
    }
    for (path, from) in [
        ("../x", None),
        ("/etc/passwd", None),
        ("a.rs", Some("../y")),
    ] {
        let args = json!({"cwd": cwd, "base": "main", "path": path, "from": from});
        let err = call(&git, ops::GIT_BRANCH_DIFF_FILE, args).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden, "{path}");
    }
    assert!(fake.commands().is_empty());
}

#[test]
fn a_base_that_does_not_exist_fails_with_its_reason() {
    let t = TempDir::new("gitops-branch-missing");
    t.write("proj/a.rs", "x");
    let (git, fake) = rig(vec![t.path("proj")]);
    fake.push_output(Ok(LaunchOutput::ok(&z(&["# branch.head f"]))));
    fake.push_output(Ok(LaunchOutput::failed(128, "")));
    let cwd = t.path("proj").to_string_lossy().into_owned();
    let err = call(
        &git,
        ops::GIT_BRANCH_DIFF_STAT,
        json!({"cwd": cwd, "base": "nope"}),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert_eq!(err.message, "no such branch");
}

#[test]
fn a_system_that_cannot_sandbox_git_answers_unimplemented_for_the_branch_ops() {
    let t = TempDir::new("gitops-branch-unsupported");
    t.write("proj/a.rs", "x");
    let cwd = t.path("proj").to_string_lossy().into_owned();
    for op in [ops::GIT_BRANCH_DIFF_STAT, ops::GIT_BRANCHES] {
        let fake = Arc::new(FakeLauncher::new());
        fake.push_output(Err(LaunchError::Unsupported));
        let known = vec![t.path("proj")];
        let git = GitOps::new(fake, Arc::new(move || known.clone()));
        let err = call(&git, op, json!({"cwd": cwd})).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unimplemented, "{op}");
    }
}

#[cfg(unix)]
#[test]
fn a_real_branch_reads_through_the_branch_ops() {
    use notchling_engine::control::RealLauncher;
    let t = TempDir::new("gitops-branch-real");
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
    assert!(git_in(&["init", "-q", "-b", "main"]));
    t.write("proj/a.txt", "one\n");
    t.write("proj/gone.txt", "bye\n");
    assert!(git_in(&["add", "."]));
    assert!(git_in(&["commit", "-q", "-m", "first"]));
    assert!(git_in(&["checkout", "-q", "-b", "feature"]));
    t.write("proj/a.txt", "one\ntwo\n");
    t.write("proj/b.txt", "new\n");
    assert!(git_in(&["rm", "-q", "gone.txt"]));
    assert!(git_in(&["add", "."]));
    assert!(git_in(&["commit", "-q", "-m", "work"]));
    t.write("proj/c.txt", "loose\n");
    let real = RealLauncher::with_claude(PathBuf::from("/nonexistent"));
    #[cfg(target_os = "macos")]
    let launcher: Arc<dyn Launcher> = Arc::new(SandboxedLauncher::waiting(
        Arc::new(real),
        Duration::from_secs(60),
    ));
    #[cfg(not(target_os = "macos"))]
    let launcher: Arc<dyn Launcher> = Arc::new(real);
    let known = vec![proj.clone()];
    let ops_ = GitOps::new(launcher, Arc::new(move || known.clone()));
    let cwd = proj.to_string_lossy().into_owned();
    let stat = call(&ops_, ops::GIT_BRANCH_DIFF_STAT, json!({"cwd": cwd})).unwrap();
    assert_eq!(stat["branch"], "feature");
    assert_eq!(stat["base"], "main");
    assert_eq!(stat["file_count"], 4);
    assert_eq!(
        (stat["added"].as_u64(), stat["removed"].as_u64()),
        (Some(3), Some(1))
    );
    assert_eq!(stat["uncommitted_files"], 1);
    let menu = call(&ops_, ops::GIT_BRANCHES, json!({"cwd": cwd})).unwrap();
    assert_eq!(menu["current"], "feature");
    assert_eq!(menu["default"], "main");
    assert_eq!(menu["local"].as_array().unwrap().len(), 2);
    let args = json!({"cwd": cwd, "base": "main", "path": "a.txt"});
    let file = call(&ops_, ops::GIT_BRANCH_DIFF_FILE, args).unwrap();
    assert!(file["text"].as_str().unwrap().contains("+two"));
    assert_eq!(
        (file["truncated"].clone(), file["untracked"].clone()),
        (json!(false), json!(false))
    );
    let args = json!({"cwd": cwd, "base": "main", "path": "c.txt"});
    let loose = call(&ops_, ops::GIT_BRANCH_DIFF_FILE, args).unwrap();
    assert_eq!(loose["untracked"], true);
}
