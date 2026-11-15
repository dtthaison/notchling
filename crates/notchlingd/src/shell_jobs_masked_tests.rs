#![cfg(unix)]

use super::{Reporter, ShellGrants, ShellJobs};
use crate::cluster_redact::{JobMasks, TailMasks};
use crate::cluster_testkit::Dir;
use crate::grant_roots::JobRoots;
use notchling_engine::cluster::grant_files::GrantValue;
use notchling_engine::cluster::JobState;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const JOB: &str = "j-0123456789ab";

#[derive(Default)]
struct Recorder {
    got: Mutex<Vec<String>>,
    cv: Condvar,
}

impl Reporter for Recorder {
    fn report(&self, job: &str, _state: JobState, _detail: Option<String>) {
        self.got.lock().unwrap().push(job.to_string());
        self.cv.notify_all();
    }
}

impl Recorder {
    fn wait(&self, job: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut got = self.got.lock().unwrap();
        while !got.iter().any(|j| j == job) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "no report for {job}");
            got = self.cv.wait_timeout(got, left).unwrap().0;
        }
    }
}

struct Rig {
    data: Dir,
    work: Dir,
    reporter: Arc<Recorder>,
    masks: Arc<TailMasks>,
    roots: Arc<JobRoots>,
    ended: Arc<AtomicUsize>,
    jobs: Arc<ShellJobs>,
}

fn rig(tag: &str) -> Rig {
    let reporter = Arc::new(Recorder::default());
    let masks = Arc::new(TailMasks::new());
    let roots = Arc::new(JobRoots::new());
    let ended = Arc::new(AtomicUsize::new(0));
    let (counted, ending) = (ended.clone(), roots.clone());
    let data = Dir::new(&format!("{tag}-data"));
    let jobs =
        ShellJobs::new(data.path().to_path_buf(), reporter.clone()).with_grants(ShellGrants {
            masks: masks.clone(),
            roots: roots.clone(),
            ended: Arc::new(move |job: &str| {
                counted.fetch_add(1, Ordering::SeqCst);
                ending.end(job);
            }),
        });
    Rig {
        data,
        work: Dir::new(&format!("{tag}-work")),
        reporter,
        masks,
        roots,
        ended,
        jobs,
    }
}

impl Rig {
    fn run(&self, command: &str) -> serde_json::Value {
        self.jobs
            .run(&json!({
                "job": JOB,
                "cwd": self.work.path().to_string_lossy(),
                "command": command,
            }))
            .expect("started")
    }

    fn log(&self) -> String {
        let path = self
            .data
            .path()
            .join("cluster")
            .join("jobs")
            .join(format!("{JOB}.log"));
        String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned()
    }

    fn grant(&self, name: &str, value: &str) {
        self.masks
            .add(
                JOB,
                vec![(name.to_string(), GrantValue::new(value.to_string()))],
            )
            .expect("added");
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.jobs.close();
    }
}

#[test]
fn shell_log_is_masked_at_write() {
    let rig = rig("masked");
    rig.grant("api-key", "NL-MARKER-1");
    rig.run("printf 'a NL-MARKER-1 b\\n'; printf 'NL-MARKER-'; sleep 0.2; printf '1 split\\n'; echo NL-MARKER-1 >&2");
    rig.reporter.wait(JOB);
    let log = rig.log();
    assert!(!log.contains("NL-MARKER"), "no value reached the log");
    assert_eq!(log.matches("[secret api-key]").count(), 3);
    assert!(log.contains("a [secret api-key] b\n"));
}

#[test]
fn job_without_grants_is_logged_as_is() {
    let rig = rig("plain");
    rig.run("printf 'out\\n'; printf 'err\\n' >&2");
    rig.reporter.wait(JOB);
    let log = rig.log();
    assert!(log.contains("out\n") && log.contains("err\n"));
    assert_eq!(rig.masks.state(JOB), JobMasks::Never);
}

#[test]
fn a_grant_that_arrives_mid_job_masks_from_then_on() {
    let rig = rig("late");
    rig.run("sleep 0.4; echo NL-MARKER-1");
    std::thread::sleep(Duration::from_millis(100));
    rig.grant("api-key", "NL-MARKER-1");
    rig.reporter.wait(JOB);
    let log = rig.log();
    assert!(!log.contains("NL-MARKER"));
    assert!(log.contains("[secret api-key]"));
}

#[test]
fn a_job_with_masks_gone_logs_nothing() {
    let rig = rig("gone");
    rig.masks.mark_granted(JOB);
    rig.run("echo NL-MARKER-1; echo more");
    rig.reporter.wait(JOB);
    assert!(rig.log().is_empty(), "output isn't kept without its masks");
}

#[test]
fn the_root_is_recorded_and_the_end_is_announced_before_the_report() {
    let rig = rig("root");
    let started = rig.run("sleep 0.3");
    let pid = u32::try_from(started["pid"].as_u64().unwrap()).unwrap();
    assert_eq!(rig.roots.rooted(), [JOB.to_string()]);
    let procs = crate::grant_use::tests::Tree::default();
    assert_eq!(rig.roots.job_of(&procs, pid).as_deref(), Some(JOB));
    assert_eq!(
        rig.ended.load(Ordering::SeqCst),
        0,
        "not ended while it runs"
    );
    rig.reporter.wait(JOB);
    assert_eq!(rig.ended.load(Ordering::SeqCst), 1);
    assert!(rig.roots.is_ended(JOB));
    assert!(rig.roots.rooted().is_empty());
    assert!(rig.jobs.log_closed(JOB));
    assert!(
        rig.jobs.log_closed("j-ffffffffffff"),
        "an unknown job has no open log"
    );
}

#[test]
fn a_leaked_child_holding_the_output_does_not_hold_the_report() {
    let rig = rig("leak");
    rig.grant("api-key", "NL-MARKER-1");
    let began = Instant::now();
    rig.run("(sleep 4 &) ; echo done");
    rig.reporter.wait(JOB);
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "the report waited too long"
    );
    assert!(!rig.log().contains("NL-MARKER"));
}

#[test]
fn a_process_in_the_job_tree_is_found_by_the_real_process_table() {
    let rig = rig("tree");
    let pidfile = rig.work.path().join("pid");
    let command = format!("sleep 5 & echo $! > '{}'; wait", pidfile.display());
    let started = rig.run(&command);
    let root = u32::try_from(started["pid"].as_u64().unwrap()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let child = loop {
        if let Some(pid) = std::fs::read_to_string(&pidfile)
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())
        {
            break pid;
        }
        assert!(Instant::now() < deadline, "no pid file");
        std::thread::sleep(Duration::from_millis(20));
    };
    let procs = crate::vault_caller::SystemProcesses;
    assert_eq!(rig.roots.job_of(&procs, child).as_deref(), Some(JOB));
    assert_eq!(rig.roots.job_of(&procs, std::process::id()), None);
    assert_ne!(child, root);
}
