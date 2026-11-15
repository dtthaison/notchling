#![cfg(unix)]

use super::{Reporter, ShellJobs};
use crate::cluster_testkit::Dir;
use notchling_engine::cluster::JobState;
use notchling_local_api::ErrorCode;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Recorder {
    got: Mutex<Vec<(String, JobState, Option<String>)>>,
    cv: Condvar,
}

impl Reporter for Recorder {
    fn report(&self, job: &str, state: JobState, detail: Option<String>) {
        self.got
            .lock()
            .unwrap()
            .push((job.to_string(), state, detail));
        self.cv.notify_all();
    }
}

impl Recorder {
    fn wait(&self, job: &str) -> (JobState, Option<String>) {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut got = self.got.lock().unwrap();
        loop {
            if let Some((_, state, detail)) = got.iter().find(|(j, _, _)| j == job) {
                return (*state, detail.clone());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "no report for {job}");
            got = self.cv.wait_timeout(got, left).unwrap().0;
        }
    }

    fn count(&self) -> usize {
        self.got.lock().unwrap().len()
    }
}

struct Rig {
    data: Dir,
    work: Dir,
    reporter: Arc<Recorder>,
    jobs: Arc<ShellJobs>,
}

fn rig(tag: &str) -> Rig {
    let reporter = Arc::new(Recorder::default());
    let data = Dir::new(&format!("{tag}-data"));
    let work = Dir::new(&format!("{tag}-work"));
    let jobs = ShellJobs::new(data.path().to_path_buf(), reporter.clone())
        .with_grace(Duration::from_millis(300));
    Rig {
        data,
        work,
        reporter,
        jobs,
    }
}

impl Rig {
    fn run(&self, job: &str, command: &str) -> Result<Value, notchling_local_api::ApiError> {
        self.jobs.run(&json!({
            "job": job,
            "cwd": self.work.path().to_string_lossy(),
            "command": command,
        }))
    }

    fn log(&self, job: &str) -> std::path::PathBuf {
        self.data
            .path()
            .join("cluster")
            .join("jobs")
            .join(format!("{job}.log"))
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.jobs.close();
    }
}

fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

fn wait_for_file(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(path) {
            if text.ends_with('\n') {
                return text;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("no {}", path.display());
}

fn wait_gone(pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(pid), "{pid} is still running");
}

#[test]
fn shell_job_runs_and_logs() {
    let rig = rig("run");
    let started = rig.run("j-1", "printf hi").unwrap();
    assert_eq!(started["session"], "shell:j-1");
    assert_eq!(started["id"], "shell:j-1");
    let (state, detail) = rig.reporter.wait("j-1");
    assert_eq!(state, JobState::Done);
    assert_eq!(detail, None);
    assert_eq!(std::fs::read_to_string(rig.log("j-1")).unwrap(), "hi");
    let mode = std::fs::metadata(rig.log("j-1"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let dir = std::fs::metadata(rig.log("j-1").parent().unwrap())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(dir & 0o077, 0, "the jobs folder is private");
    let rows = rig.jobs.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "shell:j-1");
    assert_eq!(rows[0]["tool"], "shell");
    assert_eq!(rows[0]["group"], "completed");
    assert_eq!(rows[0]["label"], "Done");
    assert_eq!(rows[0]["command"], "printf hi");
    assert_eq!(
        rows[0]["transcriptPath"],
        json!(rig.log("j-1").to_string_lossy())
    );
    assert!(serde_json::from_value::<notchling_engine::sessions::Agent>(rows[0].clone()).is_ok());
}

#[test]
fn a_failing_command_reports_its_exit_code_and_both_streams_reach_the_log() {
    let rig = rig("fail");
    rig.run("j-2", "echo out; echo err >&2; exit 3").unwrap();
    let (state, detail) = rig.reporter.wait("j-2");
    assert_eq!(state, JobState::Failed);
    assert_eq!(detail.as_deref(), Some("exit 3"));
    let log = std::fs::read_to_string(rig.log("j-2")).unwrap();
    assert!(log.contains("out") && log.contains("err"), "{log}");
    assert_eq!(rig.jobs.rows()[0]["label"], "Failed");
}

#[test]
fn shell_stop_kills_the_group() {
    let rig = rig("stop");
    let pids = rig.work.path().join("pids");
    rig.run("j-3", "sleep 60 & echo $! > pids; wait").unwrap();
    let child: i32 = wait_for_file(&pids).trim().parse().unwrap();
    assert!(alive(child));
    assert_eq!(rig.jobs.rows()[0]["group"], "working");
    let stopped = rig.jobs.stop(&json!({"id": "shell:j-3"})).unwrap();
    assert_eq!(stopped["stopped"], true);
    let (state, _) = rig.reporter.wait("j-3");
    assert_eq!(state, JobState::Interrupted);
    wait_gone(child);
    assert_eq!(rig.jobs.rows()[0]["label"], "Stopped");
    let again = rig.jobs.stop(&json!({"job": "j-3"})).unwrap();
    assert_eq!(again["stopped"], false);
    assert_eq!(rig.reporter.count(), 1, "one report per job");
}

#[test]
fn a_job_that_ignores_term_is_killed_after_the_grace() {
    let rig = rig("kill");
    let pids = rig.work.path().join("pids");
    rig.run("j-4", "trap '' TERM; sleep 60 & echo $! > pids; wait")
        .unwrap();
    let child: i32 = wait_for_file(&pids).trim().parse().unwrap();
    let began = Instant::now();
    rig.jobs.stop(&json!({"id": "shell:j-4"})).unwrap();
    let (state, _) = rig.reporter.wait("j-4");
    assert_eq!(state, JobState::Interrupted);
    assert!(began.elapsed() >= Duration::from_millis(250));
    wait_gone(child);
}

#[test]
fn the_environment_is_scrubbed_of_notchling_and_claude_code_variables() {
    let rig = rig("env");
    std::env::set_var("NOTCHLING_T13_PROBE", "leak");
    std::env::set_var("CLAUDE_CODE_T13_PROBE", "leak");
    std::env::set_var("T13_KEPT_PROBE", "kept");
    rig.run("j-5", "env").unwrap();
    rig.reporter.wait("j-5");
    let log = std::fs::read_to_string(rig.log("j-5")).unwrap();
    assert!(!log.contains("NOTCHLING_T13_PROBE"), "{log}");
    assert!(!log.contains("CLAUDE_CODE_T13_PROBE"), "{log}");
    assert!(log.contains("T13_KEPT_PROBE=kept"), "{log}");
}

#[test]
fn the_job_runs_in_the_folder_it_was_given() {
    let rig = rig("cwd");
    rig.run("j-6", "pwd").unwrap();
    rig.reporter.wait("j-6");
    let log = std::fs::read_to_string(rig.log("j-6")).unwrap();
    let real = std::fs::canonicalize(rig.work.path()).unwrap();
    assert_eq!(Path::new(log.trim()), real);
}

#[test]
fn a_bad_request_starts_nothing() {
    let rig = rig("bad");
    let missing = rig.jobs.run(&json!({
        "job": "j-7", "cwd": "/no/such/folder-t13", "command": "true"
    }));
    assert_eq!(missing.unwrap_err().code, ErrorCode::BadRequest);
    let relative = rig
        .jobs
        .run(&json!({"job": "j-7", "cwd": "rel", "command": "true"}));
    assert_eq!(
        relative.unwrap_err().message,
        "the folder must be an absolute path"
    );
    let escape = rig.run("../x", "true");
    assert_eq!(escape.unwrap_err().message, "not a job name");
    let none = rig.jobs.run(&json!({"cwd": "/tmp"}));
    assert_eq!(none.unwrap_err().code, ErrorCode::BadRequest);
    assert!(rig.jobs.rows().is_empty());
    assert_eq!(rig.reporter.count(), 0);
}

#[test]
fn a_planted_link_is_never_followed() {
    let rig = rig("link");
    let target = rig.work.path().join("victim");
    std::fs::write(&target, "keep").unwrap();
    let jobs = rig.data.path().join("cluster/jobs");
    std::fs::create_dir_all(&jobs).unwrap();
    std::os::unix::fs::symlink(&target, jobs.join("j-8.log")).unwrap();
    assert!(rig.run("j-8", "printf pwned").is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");

    let other = Dir::new("link-other");
    let rig2 = self::rig("link2");
    std::fs::create_dir_all(rig2.data.path().join("cluster")).unwrap();
    std::os::unix::fs::symlink(other.path(), rig2.data.path().join("cluster/jobs")).unwrap();
    assert!(rig2.run("j-9", "printf pwned").is_err());
    assert!(std::fs::read_dir(other.path()).unwrap().next().is_none());
}

#[test]
fn the_same_job_twice_is_one_process() {
    let rig = rig("twice");
    let first = rig.run("j-10", "sleep 60").unwrap();
    let again = rig.run("j-10", "sleep 60").unwrap();
    assert_eq!(first["session"], again["session"]);
    assert_eq!(rig.jobs.rows().len(), 1);
}

#[test]
fn stopping_an_unknown_job_is_an_error() {
    let rig = rig("unknown");
    let e = rig.jobs.stop(&json!({"id": "shell:nope"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    let e = rig.jobs.stop(&json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
}

#[test]
fn closing_stops_what_still_runs_and_reaps_it() {
    let rig = rig("close");
    let pids = rig.work.path().join("pids");
    rig.run("j-11", "sleep 60 & echo $! > pids; wait").unwrap();
    let child: i32 = wait_for_file(&pids).trim().parse().unwrap();
    rig.jobs.close();
    wait_gone(child);
    assert_eq!(rig.jobs.rows()[0]["group"], "completed");
}

#[test]
fn old_finished_jobs_are_dropped_from_the_list() {
    let rig = rig("many");
    for n in 0..(super::KEEP + 5) {
        rig.run(&format!("j-m{n}"), "true").unwrap();
        rig.reporter.wait(&format!("j-m{n}"));
    }
    assert_eq!(rig.jobs.rows().len(), super::KEEP);
}
