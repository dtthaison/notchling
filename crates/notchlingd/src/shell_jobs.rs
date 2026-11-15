use crate::acting_ops::now_ms;
use notchling_engine::cluster::shell_job::{scrub, shell_plan, ShellPlan};
use notchling_engine::cluster::JobState;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

pub const KEEP: usize = 50;
pub const GRACE: Duration = Duration::from_secs(2);
pub const SESSION_PREFIX: &str = "shell:";
const NAME_MAX: usize = 60;

pub trait Reporter: Send + Sync {
    fn report(&self, job: &str, state: JobState, detail: Option<String>);
}

type Done = Arc<(Mutex<bool>, Condvar)>;

struct Entry {
    command: String,
    cwd: String,
    log: PathBuf,
    pid: u32,
    started_ms: i64,
    ended: Option<(i64, JobState, Option<String>)>,
    stopping: bool,
    done: Done,
}

struct Inner {
    data: PathBuf,
    reporter: Arc<dyn Reporter>,
    grace_ms: AtomicU64,
    jobs: Mutex<BTreeMap<String, Entry>>,
    watchers: Mutex<Vec<JoinHandle<()>>>,
}

pub struct ShellJobs {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn bad(why: impl Into<String>) -> ApiError {
    ApiError::bad_request(why)
}

fn failed(why: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, why)
}

fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("{key} is needed")))
}

fn reply(job: &str, pid: u32) -> Value {
    let id = format!("{SESSION_PREFIX}{job}");
    json!({"id": id, "session": id, "job": job, "pid": pid})
}

fn real_dir(path: &Path) -> std::io::Result<()> {
    let refuse = || std::io::Error::other("a job folder is a link or not a folder");
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(refuse()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            notchling_platform::files::ensure_private_dir(path).map_err(std::io::Error::other)?;
            match std::fs::symlink_metadata(path) {
                Ok(meta) if meta.is_dir() => Ok(()),
                _ => Err(refuse()),
            }
        }
        Err(e) => Err(e),
    }
}

fn open_log(log: &Path) -> std::io::Result<File> {
    let jobs = log
        .parent()
        .ok_or_else(|| std::io::Error::other("no jobs folder"))?;
    let cluster = jobs
        .parent()
        .ok_or_else(|| std::io::Error::other("no cluster folder"))?;
    real_dir(cluster)?;
    real_dir(jobs)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(log)
}

#[cfg(unix)]
fn launch(plan: &ShellPlan, cwd: &str, log: &File) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut command = Command::new(&plan.argv[0]);
    command
        .args(&plan.argv[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?)
        .process_group(0);
    for (name, _) in std::env::vars_os() {
        let lossy = name.to_string_lossy().into_owned();
        if !scrub([lossy], &plan.env_remove).is_empty() {
            command.env_remove(&name);
        }
    }
    command.spawn()
}

#[cfg(not(unix))]
fn launch(_plan: &ShellPlan, _cwd: &str, _log: &File) -> std::io::Result<std::process::Child> {
    Err(std::io::Error::other("shell jobs need a Unix shell"))
}

#[cfg(unix)]
fn signal_group(pid: u32, signal: i32) {
    if let Ok(pgid) = libc::pid_t::try_from(pid) {
        // SAFETY: signalling the process group of a child this module started.
        unsafe { libc::killpg(pgid, signal) };
    }
}

#[cfg(not(unix))]
fn signal_group(_pid: u32, _signal: i32) {}

#[cfg(unix)]
const TERM: i32 = libc::SIGTERM;
#[cfg(unix)]
const KILL: i32 = libc::SIGKILL;
#[cfg(not(unix))]
const TERM: i32 = 15;
#[cfg(not(unix))]
const KILL: i32 = 9;

#[cfg(unix)]
fn classify(status: &std::process::ExitStatus, stopping: bool) -> (JobState, Option<String>) {
    use std::os::unix::process::ExitStatusExt;
    if status.success() {
        return (JobState::Done, None);
    }
    if stopping {
        return (JobState::Interrupted, Some("stopped".to_string()));
    }
    match (status.code(), status.signal()) {
        (Some(code), _) => (JobState::Failed, Some(format!("exit {code}"))),
        (None, Some(signal)) => (JobState::Failed, Some(format!("signal {signal}"))),
        _ => (JobState::Failed, None),
    }
}

#[cfg(not(unix))]
fn classify(status: &std::process::ExitStatus, stopping: bool) -> (JobState, Option<String>) {
    if status.success() {
        (JobState::Done, None)
    } else if stopping {
        (JobState::Interrupted, Some("stopped".to_string()))
    } else {
        (JobState::Failed, status.code().map(|c| format!("exit {c}")))
    }
}

fn watch(inner: Arc<Inner>, job: String, mut child: std::process::Child, done: Done) {
    let status = child.wait();
    let (state, detail) = {
        let mut jobs = lock(&inner.jobs);
        let stopping = jobs.get(&job).is_some_and(|e| e.stopping);
        let outcome = match &status {
            Ok(status) => classify(status, stopping),
            Err(e) => (JobState::Failed, Some(e.to_string())),
        };
        if let Some(entry) = jobs.get_mut(&job) {
            entry.ended = Some((now_ms(), outcome.0, outcome.1.clone()));
        }
        outcome
    };
    {
        let (flag, wake) = &*done;
        *lock(flag) = true;
        wake.notify_all();
    }
    inner.reporter.report(&job, state, detail);
}

impl ShellJobs {
    pub fn new(data: PathBuf, reporter: Arc<dyn Reporter>) -> Arc<ShellJobs> {
        Arc::new(ShellJobs {
            inner: Arc::new(Inner {
                data,
                reporter,
                grace_ms: AtomicU64::new(GRACE.as_millis() as u64),
                jobs: Mutex::new(BTreeMap::new()),
                watchers: Mutex::new(Vec::new()),
            }),
        })
    }

    pub fn with_grace(self: Arc<Self>, grace: Duration) -> Arc<ShellJobs> {
        self.inner
            .grace_ms
            .store(grace.as_millis() as u64, Ordering::SeqCst);
        self
    }

    fn grace(&self) -> Duration {
        Duration::from_millis(self.inner.grace_ms.load(Ordering::SeqCst))
    }

    pub fn run(&self, args: &Value) -> Result<Value, ApiError> {
        let job = text(args, "job")?;
        let cwd = text(args, "cwd")?;
        let command = text(args, "command")?;
        let plan = shell_plan(job, cwd, command, &self.inner.data).map_err(bad)?;
        if let Some(entry) = lock(&self.inner.jobs).get(job) {
            return Ok(reply(job, entry.pid));
        }
        if !Path::new(cwd).is_dir() {
            return Err(bad(format!("no folder at {cwd}")));
        }
        let log = open_log(&plan.log).map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => failed("that job's log already exists"),
            _ => failed(format!("the job's log: {e}")),
        })?;
        let child = launch(&plan, cwd, &log).map_err(|e| failed(format!("starting: {e}")))?;
        drop(log);
        let pid = child.id();
        let done: Done = Arc::new((Mutex::new(false), Condvar::new()));
        {
            let mut jobs = lock(&self.inner.jobs);
            jobs.insert(
                job.to_string(),
                Entry {
                    command: command.to_string(),
                    cwd: cwd.to_string(),
                    log: plan.log.clone(),
                    pid,
                    started_ms: now_ms(),
                    ended: None,
                    stopping: false,
                    done: done.clone(),
                },
            );
            prune(&mut jobs);
        }
        let inner = self.inner.clone();
        let name = job.to_string();
        let spawned = std::thread::Builder::new()
            .name("shell-job".into())
            .spawn(move || watch(inner, name, child, done));
        match spawned {
            Ok(handle) => lock(&self.inner.watchers).push(handle),
            Err(e) => {
                signal_group(pid, KILL);
                return Err(failed(format!("no thread for the job: {e}")));
            }
        }
        Ok(reply(job, pid))
    }

    pub fn stop(&self, args: &Value) -> Result<Value, ApiError> {
        let raw = args
            .get("id")
            .or_else(|| args.get("job"))
            .and_then(Value::as_str)
            .ok_or_else(|| bad("shell.stop takes {\"id\": \"shell:<job>\"}"))?;
        let job = raw.strip_prefix(SESSION_PREFIX).unwrap_or(raw);
        let (pid, done) = {
            let mut jobs = lock(&self.inner.jobs);
            let entry = jobs
                .get_mut(job)
                .ok_or_else(|| failed("no such shell job"))?;
            if entry.ended.is_some() || entry.stopping {
                return Ok(json!({"stopped": false}));
            }
            entry.stopping = true;
            (entry.pid, entry.done.clone())
        };
        self.terminate(pid, done);
        Ok(json!({"stopped": true}))
    }

    fn terminate(&self, pid: u32, done: Done) {
        signal_group(pid, TERM);
        let grace = self.grace();
        let _ = std::thread::Builder::new()
            .name("shell-kill".into())
            .spawn(move || {
                let (flag, wake) = &*done;
                let finished = lock(flag);
                let (finished, timeout) = wake
                    .wait_timeout_while(finished, grace, |gone| !*gone)
                    .unwrap_or_else(|e| e.into_inner());
                if timeout.timed_out() && !*finished {
                    signal_group(pid, KILL);
                }
            });
    }

    pub fn close(&self) {
        let running: Vec<(u32, Done)> = {
            let mut jobs = lock(&self.inner.jobs);
            jobs.values_mut()
                .filter(|e| e.ended.is_none() && !e.stopping)
                .map(|e| {
                    e.stopping = true;
                    (e.pid, e.done.clone())
                })
                .collect()
        };
        for (pid, done) in running {
            self.terminate(pid, done);
        }
        let watchers = std::mem::take(&mut *lock(&self.inner.watchers));
        for handle in watchers {
            let _ = handle.join();
        }
    }

    pub fn rows(&self) -> Vec<Value> {
        let jobs = lock(&self.inner.jobs);
        jobs.iter().filter_map(|(job, e)| row(job, e)).collect()
    }
}

fn prune(jobs: &mut BTreeMap<String, Entry>) {
    while jobs.len() > KEEP {
        let oldest = jobs
            .iter()
            .filter(|(_, e)| e.ended.is_some())
            .min_by_key(|(_, e)| e.started_ms)
            .map(|(job, _)| job.clone());
        match oldest {
            Some(job) => jobs.remove(&job),
            None => return,
        };
    }
}

fn row(job: &str, e: &Entry) -> Option<Value> {
    let (group, label, detail, since_ms) = match &e.ended {
        None => (AgentGroup::Working, "Running", None, e.started_ms),
        Some((at, JobState::Done, _)) => (AgentGroup::Completed, "Done", None, *at),
        Some((at, JobState::Interrupted, why)) => {
            (AgentGroup::Completed, "Stopped", why.clone(), *at)
        }
        Some((at, _, why)) => (AgentGroup::Completed, "Failed", why.clone(), *at),
    };
    let first = e.command.lines().next().unwrap_or_default();
    let name: String = first.chars().take(NAME_MAX).collect();
    let agent = Agent {
        id: format!("{SESSION_PREFIX}{job}"),
        name,
        project: Path::new(&e.cwd)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        cwd: e.cwd.clone(),
        group,
        label: label.to_string(),
        detail,
        since_ms,
        target: AgentTarget::Process(e.pid),
        suggested_reply: None,
        transcript_path: Some(e.log.to_string_lossy().into_owned()),
        host: "Shell".to_string(),
        headless: false,
        config_dir: None,
        command: Some(e.command.clone()),
        resume_session_id: None,
        at_prompt: false,
        node: None,
    };
    let mut value = serde_json::to_value(agent).ok()?;
    value["tool"] = json!("shell");
    Some(value)
}

#[cfg(test)]
#[path = "shell_jobs_tests.rs"]
mod tests;
