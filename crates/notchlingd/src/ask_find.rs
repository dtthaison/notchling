use crate::ask_ops::AskOps;
use crate::caller_session::SessionHit;
use crate::vault_caller::Processes;
use notchling_engine::ask::{
    candidates, delegate_tool, find, job_label, Asker, Asking, Candidate, Found, FIND_MS,
    FIND_TAIL_BYTES,
};
use notchling_engine::transcript::locate::find_transcript;
use notchling_events::kinds::choices::AskThen;
use notchling_platform::watch::{Batch, WatchConfig, Watcher};
use std::any::Any;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const SLICE: Duration = Duration::from_millis(25);
const POLL_MS: i64 = 500;
const NOW_SLICE: Duration = Duration::from_millis(100);
const WATCH: WatchConfig = WatchConfig {
    poll: Duration::from_millis(250),
    debounce: Duration::from_millis(100),
    max_latency: Duration::from_millis(300),
};

pub(crate) type Reader = Arc<dyn Fn(&Path) -> Option<String> + Send + Sync>;
pub(crate) type Watching = Arc<dyn Fn(Vec<PathBuf>) -> Option<Subscription> + Send + Sync>;

pub(crate) struct Subscription {
    rx: Receiver<Batch>,
    _keep: Box<dyn Any>,
}

impl Subscription {
    pub(crate) fn new(rx: Receiver<Batch>, keep: Box<dyn Any>) -> Subscription {
        Subscription { rx, _keep: keep }
    }

    fn changed(&self) -> bool {
        let mut any = false;
        while self.rx.try_recv().is_ok() {
            any = true;
        }
        any
    }
}

pub(crate) struct Seams {
    pub read: Reader,
    pub watch: Watching,
    pub exe: PathBuf,
    pub main_find_ms: i64,
}

impl Seams {
    pub(crate) fn system() -> Seams {
        Seams {
            read: Arc::new(tail_text),
            watch: Arc::new(system_watch),
            exe: std::env::current_exe().unwrap_or_default(),
            main_find_ms: notchling_engine::ask::MAIN_FIND_MS,
        }
    }
}

fn system_watch(folders: Vec<PathBuf>) -> Option<Subscription> {
    let (watcher, rx) = Watcher::start(folders, WATCH).ok()?;
    Some(Subscription::new(rx, Box::new(watcher)))
}

fn tail_text(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(FIND_TAIL_BYTES)))
        .ok()?;
    let mut bytes = Vec::new();
    file.take(FIND_TAIL_BYTES).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

enum Cmd {
    Start(Start),
    Stop,
}

struct Start {
    id: String,
    hit: SessionHit,
    pid: u32,
    opened_ms: i64,
}

struct Job {
    id: String,
    hit: SessionHit,
    pid: u32,
    opened_ms: i64,
    deadline_ms: i64,
    transcript: Option<PathBuf>,
    sub: Option<Subscription>,
    first: bool,
    next_poll_ms: i64,
    partial: Option<Found>,
}

enum Attempt {
    Done(Found),
    Partial(Found),
    Nothing,
}

pub(crate) struct Finder {
    tx: Sender<Cmd>,
    thread: Mutex<Option<JoinHandle<()>>>,
    seams: Arc<Seams>,
    ops: Weak<AskOps>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Finder {
    pub(crate) fn spawn(ops: Weak<AskOps>, seams: Seams) -> Finder {
        let seams = Arc::new(seams);
        let (tx, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("notchling-ask-find".into())
            .spawn({
                let (ops, seams) = (ops.clone(), seams.clone());
                move || work(ops, seams, rx)
            })
            .ok();
        Finder {
            tx,
            thread: Mutex::new(thread),
            seams,
            ops,
        }
    }

    pub(crate) fn start(&self, id: String, hit: SessionHit, pid: u32, opened_ms: i64) {
        let _ = self.tx.send(Cmd::Start(Start {
            id,
            hit,
            pid,
            opened_ms,
        }));
    }

    pub(crate) fn stop(&self) {
        let thread = lock(&self.thread).take();
        if let Some(handle) = thread {
            let _ = self.tx.send(Cmd::Stop);
            let _ = handle.join();
        }
    }

    pub(crate) fn find_main(&self, hit: &SessionHit, pid: u32) -> Option<Found> {
        self.find_now(hit, pid, self.seams.main_find_ms)
    }

    pub(crate) fn find_now(&self, hit: &SessionHit, pid: u32, within_ms: i64) -> Option<Found> {
        let ops = self.ops.upgrade()?;
        let stop = Instant::now() + Duration::from_millis(within_ms.max(0) as u64);
        let mut sub: Option<Subscription> = None;
        let mut watched = false;
        loop {
            if let Some(transcript) = locate(&ops, hit, pid) {
                if !watched {
                    watched = true;
                    sub = watch_folder(&self.seams, &transcript);
                }
                let all = candidates(&transcript, &session_name(&ops, hit), ops.now());
                let looked = resolve(
                    &*ops.procs,
                    &self.seams,
                    hit,
                    pid,
                    ops.now(),
                    &all,
                    &HashSet::new(),
                );
                match looked {
                    Attempt::Done(found) | Attempt::Partial(found) => return Some(found),
                    Attempt::Nothing => {}
                }
            }
            let left = stop.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            match &sub {
                Some(sub) => {
                    let _ = sub.rx.recv_timeout(left.min(NOW_SLICE));
                }
                None => std::thread::sleep(left.min(NOW_SLICE)),
            }
        }
    }
}

fn work(ops: Weak<AskOps>, seams: Arc<Seams>, rx: Receiver<Cmd>) {
    let mut jobs: Vec<Job> = Vec::new();
    loop {
        let cmd = if jobs.is_empty() {
            rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            rx.recv_timeout(SLICE)
        };
        match cmd {
            Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Cmd::Start(start)) => {
                let Some(ops) = ops.upgrade() else {
                    return;
                };
                jobs.push(begin(&ops, &seams, start));
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let Some(ops) = ops.upgrade() else {
            return;
        };
        jobs.retain_mut(|job| !step(&ops, &seams, job));
    }
}

fn begin(ops: &AskOps, seams: &Seams, start: Start) -> Job {
    let transcript = locate(ops, &start.hit, start.pid);
    let sub = transcript
        .as_deref()
        .and_then(|path| watch_folder(seams, path));
    Job {
        id: start.id,
        hit: start.hit,
        pid: start.pid,
        deadline_ms: start.opened_ms + FIND_MS,
        opened_ms: start.opened_ms,
        transcript,
        sub,
        first: true,
        next_poll_ms: start.opened_ms,
        partial: None,
    }
}

fn watch_folder(seams: &Seams, transcript: &Path) -> Option<Subscription> {
    let folder = transcript.parent()?.to_path_buf();
    (seams.watch)(vec![folder])
}

fn step(ops: &AskOps, seams: &Seams, job: &mut Job) -> bool {
    if !matches!(ops.store.get(&job.id), Ok(Some(row)) if row.state == notchling_engine::ask::AskState::Open)
    {
        return true;
    }
    let now = ops.now();
    let event = job.sub.as_ref().is_some_and(Subscription::changed);
    let polling = job.sub.is_none() && now >= job.next_poll_ms;
    let late = now >= job.deadline_ms;
    if job.first || event || polling || late {
        job.first = false;
        job.next_poll_ms = now + POLL_MS;
        match attempt(ops, seams, job) {
            Attempt::Done(found) => {
                finish(ops, &job.id, Some(found));
                return true;
            }
            Attempt::Partial(found) => job.partial = Some(found),
            Attempt::Nothing => {}
        }
    }
    if late {
        finish(ops, &job.id, job.partial.take());
        return true;
    }
    false
}

fn finish(ops: &AskOps, id: &str, found: Option<Found>) {
    if let Some(found) = found {
        if matches!(ops.store.set_found(id, &found, ops.now()), Ok(true)) {
            if let Ok(Some(row)) = ops.store.get(id) {
                ops.publish_changed(&row);
                ops.emit_then(&row, AskThen::Found);
            }
        }
    }
    ops.settled(id);
}

fn locate(ops: &AskOps, hit: &SessionHit, pid: u32) -> Option<PathBuf> {
    let projects = hit.config_dir.join("projects");
    let cwd = ops
        .procs
        .cwd(pid)
        .or_else(|| ops.procs.cwd(hit.claude_pid))
        .unwrap_or_default();
    find_transcript(
        &projects.to_string_lossy(),
        &hit.session_id,
        &cwd.to_string_lossy(),
    )
    .map(PathBuf::from)
}

fn session_name(ops: &AskOps, hit: &SessionHit) -> String {
    ops.session_name(&hit.session_id)
}

fn attempt(ops: &AskOps, seams: &Seams, job: &mut Job) -> Attempt {
    if job.transcript.is_none() {
        job.transcript = locate(ops, &job.hit, job.pid);
    }
    let Some(transcript) = job.transcript.as_deref() else {
        return Attempt::Nothing;
    };
    let all = candidates(transcript, &session_name(ops, &job.hit), ops.now());
    let claimed = ops.claimed(&job.id);
    resolve(
        &*ops.procs,
        seams,
        &job.hit,
        job.pid,
        job.opened_ms,
        &all,
        &claimed,
    )
}

fn same_file(procs: &dyn Processes, a: &Path, b: &Path) -> bool {
    a == b || matches!((procs.resolve(a), procs.resolve(b)), (Some(x), Some(y)) if x == y)
}

fn delegate_of(procs: &dyn Processes, seams: &Seams, hit: &SessionHit) -> Option<(Asker, u32)> {
    hit.chain.iter().find_map(|pid| {
        let exe = procs.exe(*pid)?;
        if !same_file(procs, &exe, &seams.exe) {
            return None;
        }
        let tool = delegate_tool(&procs.argv(*pid)?)?;
        Some((tool, *pid))
    })
}

fn look(
    procs: &dyn Processes,
    seams: &Seams,
    pid: u32,
    opened_ms: i64,
    all: &[Candidate],
    claimed: &HashSet<String>,
) -> Option<Found> {
    let argv = procs.argv(pid)?;
    let stdout = procs.stdout_path(pid);
    let asking = Asking {
        argv: &argv,
        stdout: stdout.as_deref(),
        opened_ms,
        claimed,
    };
    find(&asking, all, |path| (seams.read)(path))
}

fn resolve(
    procs: &dyn Processes,
    seams: &Seams,
    hit: &SessionHit,
    pid: u32,
    opened_ms: i64,
    all: &[Candidate],
    claimed: &HashSet<String>,
) -> Attempt {
    let job = |label: Option<String>, tool: Asker| Found {
        asker: tool,
        agent_id: None,
        label,
        agent_type: None,
        model: None,
        tool_use_id: None,
    };
    match delegate_of(procs, seams, hit) {
        Some((tool, delegate)) => match look(procs, seams, delegate, opened_ms, all, claimed) {
            Some(found) if found.asker != Asker::Unknown => {
                Attempt::Done(job(Some(job_label(tool, found.label.as_deref())), tool))
            }
            _ => Attempt::Partial(job(None, tool)),
        },
        None => match look(procs, seams, pid, opened_ms, all, claimed) {
            Some(found) if found.asker != Asker::Unknown => Attempt::Done(found),
            _ => Attempt::Nothing,
        },
    }
}

impl AskOps {
    pub fn attach_finder(self: &Arc<Self>) {
        self.attach_finder_with(Seams::system());
    }

    pub(crate) fn attach_finder_with(self: &Arc<Self>, seams: Seams) {
        let _ = self.finder.set(Finder::spawn(Arc::downgrade(self), seams));
    }

    pub(crate) fn start_find(&self, row: &notchling_engine::ask::Row, hit: &SessionHit) -> bool {
        let Some(finder) = self.finder.get() else {
            return false;
        };
        finder.start(row.id.clone(), hit.clone(), row.pid, row.created_ms);
        true
    }

    pub(crate) fn session_of(&self, pid: u32) -> Option<SessionHit> {
        self.caller.session_of(pid)
    }

    pub(crate) fn session_name(&self, session_id: &str) -> String {
        crate::ask_rows::name_for(&self.agents(), session_id)
    }

    pub(crate) fn claimed(&self, except: &str) -> HashSet<String> {
        self.store
            .live()
            .unwrap_or_default()
            .into_iter()
            .filter(|row| row.id != except)
            .filter_map(|row| row.tool_use_id)
            .collect()
    }
}
