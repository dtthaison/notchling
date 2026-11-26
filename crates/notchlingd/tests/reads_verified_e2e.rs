#![cfg(unix)]

mod common;

use common::{home, notchling, scratch, Daemon};
use notchling_engine::organizations::Organizations;
use serde_json::{json, Value};
use std::path::Path;

use common::verified::{run_helper, Session, Step, SESSION};
use std::time::Duration;

const JOB_SESSION: &str = "be50d36a-0000-4000-8000-000000000002";
const HELPER: &str = "reads_helper_runs_commands_as_the_session_process";

#[test]
fn reads_helper_runs_commands_as_the_session_process() {
    run_helper();
}

fn as_session(root: &Path, home: &Path, steps: &[&[&str]]) -> Vec<Value> {
    as_session_in(root, home, steps, None, true)
}

fn as_session_in(
    root: &Path,
    home: &Path,
    steps: &[&[&str]],
    live_folder: Option<&Path>,
    job: bool,
) -> Vec<Value> {
    let steps: Vec<Step> = steps.iter().map(|s| Step::args(s)).collect();
    Session {
        root,
        home,
        helper: HELPER,
        bin: env!("CARGO_BIN_EXE_notchling"),
        folder: live_folder,
        job,
        step_timeout: Duration::from_secs(60),
    }
    .run(&steps)
}

fn parsed(step: &Value) -> Value {
    serde_json::from_str(step["out"].as_str().unwrap_or("")).unwrap_or(Value::Null)
}

fn a_live_background_session(
    tag: &str,
    inside: bool,
) -> (common::Dir, std::path::PathBuf, std::path::PathBuf, String) {
    let dir = scratch(tag);
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let work = dir.0.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let book = Organizations::new(&root.join("notchling")).with_home(&root);
    let id = book.create("Acme", None, None).unwrap();
    book.set_folders(&id, &[work.to_string_lossy().into_owned()])
        .unwrap();
    let folder = if inside {
        work.join("app").to_string_lossy().into_owned()
    } else {
        "/nowhere/outside/any/organization".to_owned()
    };
    let job = root.join("claude/jobs/j1");
    std::fs::create_dir_all(&job).unwrap();
    std::fs::write(
        job.join("state.json"),
        json!({
            "sessionId": JOB_SESSION,
            "resumeSessionId": SESSION,
            "cwd": folder,
            "name": "work",
            "state": "running",
        })
        .to_string(),
    )
    .unwrap();
    (dir, root, home, id)
}

const READS: &[&[&str]] = &[
    &["capabilities", "--json"],
    &["orgs", "--json"],
    &["accounts", "--json"],
    &["inbox", "--json"],
    &["limits", "--json"],
    &["limits", "--tool", "codex", "--json"],
    &["sessions", "history", "--limit", "5", "--json"],
];

fn assert_answered_for_the_session(steps: &[Value], organization: &str, personal: bool) {
    assert_eq!(steps.len(), READS.len(), "{steps:?}");
    let caps = parsed(&steps[0]);
    assert_eq!(caps["caller"]["verified"], true, "{caps}");
    assert_eq!(caps["caller"]["session"], SESSION, "{caps}");
    for (i, step) in steps.iter().enumerate().skip(1) {
        let out = parsed(step);
        assert_ne!(out["reason"], "no_session", "{:?}: {out}", READS[i]);
        assert_ne!(
            out["result"]["reason"], "no_session",
            "{:?}: {out}",
            READS[i]
        );
        assert_ne!(
            out["error"]["reason"], "no_session",
            "{:?}: {out}",
            READS[i]
        );
    }
    let orgs = parsed(&steps[1]);
    assert_eq!(orgs["organization"]["id"], organization, "{orgs}");
    assert_eq!(orgs["organization"]["personal"], personal, "{orgs}");
    let accounts = parsed(&steps[2]);
    assert_eq!(accounts["organization"], organization, "{accounts}");
    let limits = parsed(&steps[4]);
    assert_eq!(limits["organization"], organization, "{limits}");
    let history = parsed(&steps[6]);
    assert!(history["sessions"].is_array(), "{history}");
    assert_eq!(history["organization"], organization, "{history}");
    assert_eq!(history["masked"], true, "{history}");
    assert_eq!(history["window_days"], 14, "{history}");
    assert!(history.get("next").is_some(), "{history}");
}

#[test]
fn a_live_session_that_the_list_knows_by_its_resume_id_reads_its_organization() {
    let (_dir, root, home, acme) = a_live_background_session("reads-verified-in", true);
    let _daemon = Daemon::start(&root, &home).warmed(&root);

    let (code, out, _) = notchling(&root, &home, &["limits", "--tool", "codex", "--json"]);
    assert_eq!(code, 1, "{out}");
    let shell: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(shell["result"]["reason"], "no_session", "{out}");
    assert_eq!(shell["error"]["reason"], "no_session", "{out}");

    let steps = as_session(&root, &home, READS);
    assert_answered_for_the_session(&steps, &acme, false);
}

#[test]
fn a_live_session_in_no_organization_folder_reads_as_personal() {
    let (_dir, root, home, _) = a_live_background_session("reads-verified-out", false);
    let _daemon = Daemon::start(&root, &home).warmed(&root);
    let steps = as_session(&root, &home, READS);
    assert_answered_for_the_session(&steps, "personal", true);
}

#[test]
fn a_live_session_with_no_row_and_no_file_folder_reads_by_its_process_folder() {
    let (dir, root, home, acme) = a_live_background_session("reads-verified-proc", false);
    std::fs::remove_dir_all(root.join("claude/jobs/j1")).unwrap();
    let live = dir.0.join("work/app");
    std::fs::create_dir_all(&live).unwrap();
    let _daemon = Daemon::start(&root, &home).warmed(&root);
    let steps = as_session_in(&root, &home, READS, Some(&live), false);
    assert_answered_for_the_session(&steps, &acme, false);
}

#[test]
fn a_live_job_the_list_lacks_reads_by_its_process_folder_not_its_launch_folder() {
    let (dir, root, home, acme) = a_live_background_session("reads-verified-job-proc", false);
    std::fs::remove_dir_all(root.join("claude/jobs/j1")).unwrap();
    let live = dir.0.join("work/app");
    std::fs::create_dir_all(&live).unwrap();
    let _daemon = Daemon::start(&root, &home).warmed(&root);
    let steps = as_session_in(&root, &home, READS, Some(&live), true);
    assert_answered_for_the_session(&steps, &acme, false);
}
