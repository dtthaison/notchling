#![cfg(unix)]

mod common;

use common::{home, notchling, scratch, Daemon};
use notchling_engine::organizations::Organizations;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

const SESSION: &str = "52afae32-0000-4000-8000-000000000001";
const JOB_SESSION: &str = "be50d36a-0000-4000-8000-000000000002";
const HELPER: &str = "reads_helper_runs_commands_as_the_session_process";

#[test]
fn reads_helper_runs_commands_as_the_session_process() {
    let Ok(bin) = std::env::var("NL_READS_E2E_BIN") else {
        return;
    };
    let mut go = String::new();
    std::io::stdin().lock().read_line(&mut go).unwrap();
    let steps: Vec<Vec<String>> =
        serde_json::from_str(&std::env::var("NL_READS_E2E_STEPS").unwrap()).unwrap();
    for step in steps {
        let out = Command::new(&bin)
            .args(&step)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        println!(
            "STEP {}",
            json!({"code": out.status.code(), "out": String::from_utf8_lossy(&out.stdout),
                   "err": String::from_utf8_lossy(&out.stderr)})
        );
    }
}

fn as_session(root: &Path, home: &Path, steps: &[&[&str]]) -> Vec<Value> {
    as_session_in(root, home, steps, None)
}

fn as_session_in(
    root: &Path,
    home: &Path,
    steps: &[&[&str]],
    live_folder: Option<&Path>,
) -> Vec<Value> {
    let claude_dir = root.join(".local/share/claude/versions/1");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let claude = claude_dir.join("claude");
    std::fs::copy(std::env::current_exe().unwrap(), &claude).unwrap();
    let mut command = Command::new(&claude);
    if let Some(folder) = live_folder {
        command.current_dir(folder);
    }
    let mut parent = command
        .args(["--exact", HELPER, "--nocapture", "--test-threads=1"])
        .env("NL_READS_E2E_BIN", env!("CARGO_BIN_EXE_notchling"))
        .env("NL_READS_E2E_STEPS", serde_json::to_string(steps).unwrap())
        .env("NOTCHLING_DATA_ROOT", root)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let sessions = root.join("claude/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let file = match live_folder {
        Some(_) => json!({"sessionId": SESSION, "pid": parent.id()}),
        None => json!({
            "sessionId": SESSION,
            "pid": parent.id(),
            "cwd": root.join("launch").to_string_lossy(),
            "jobId": "j1",
        }),
    };
    std::fs::write(
        sessions.join(format!("{}.json", parent.id())),
        file.to_string(),
    )
    .unwrap();
    writeln!(parent.stdin.take().unwrap(), "go").unwrap();
    let reader = BufReader::new(parent.stdout.take().unwrap());
    let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
    let _ = parent.wait();
    lines
        .iter()
        .filter_map(|line| line.split_once("STEP ").map(|(_, text)| text.to_owned()))
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .collect()
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
}

#[test]
fn a_live_session_that_the_list_knows_by_its_resume_id_reads_its_organization() {
    let (_dir, root, home, acme) = a_live_background_session("reads-verified-in", true);
    let _daemon = Daemon::start(&root, &home);

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
    let _daemon = Daemon::start(&root, &home);
    let steps = as_session(&root, &home, READS);
    assert_answered_for_the_session(&steps, "personal", true);
}

#[test]
fn a_live_session_with_no_row_and_no_file_folder_reads_by_its_process_folder() {
    let (dir, root, home, acme) = a_live_background_session("reads-verified-proc", false);
    std::fs::remove_dir_all(root.join("claude/jobs/j1")).unwrap();
    let live = dir.0.join("work/app");
    std::fs::create_dir_all(&live).unwrap();
    let _daemon = Daemon::start(&root, &home);
    let steps = as_session_in(&root, &home, READS, Some(&live));
    assert_answered_for_the_session(&steps, &acme, false);
}
