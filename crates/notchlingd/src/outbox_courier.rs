use crate::acting_events::{attach_why, session_ref, why_of};
use crate::acting_ops::{keep_screen, shown_box, ActingOps, NoLog};
use crate::outbox_rows::reply_via;
use crate::outbox_verify::SafeModes;
use crate::turn_gate::Guarded;
use notchling_engine::outbox::{
    AnswerWatch, Facts, Job, Kind, Row, Seen, Tried, TryEnd, Why, ANSWER_GRACE_MS,
    DIALOG_RECHECK_MS, GONE_AFTER_MS,
};
use notchling_engine::reply::exec::{execute_clearing, Clearing, Failure, Pty};
use notchling_engine::reply::plan;
use notchling_engine::sessions::jobs::{read_jobs, read_sessions};
use notchling_engine::sessions::AgentTarget;
use notchling_engine::sessions::{config_dirs, fresh, Agent, AgentGroup, Probes, Roots};
use notchling_engine::terminal::input_box;
use notchling_engine::terminal::screen_dialog;
use notchling_engine::transcript::locate::{find_in_folders, find_transcript, project_folders};
use notchling_engine::transcript::queue_state::{queue_state, QueueState};
use notchling_engine::transcript::restart;
use notchling_events::kinds::choices::FailWhy;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const CALM_GAP_MS: i64 = 3_000;
const ASKED_QUIET_MS: u64 = 400;
const ASKED_WAIT_MS: u64 = 1_500;
const MODEL_QUIET_MS: u64 = 1_500;
const MODEL_WAIT_MS: u64 = 5_000;

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub fn real_clock() -> Clock {
    Arc::new(crate::acting_ops::now_ms)
}

pub trait Courier: Send + Sync {
    fn facts(&self, row: &Row, now_ms: i64) -> Facts;
    fn deliver(&self, row: &Row, guarded: bool) -> (TryEnd, Tried, Option<String>);
    fn close(&self, session: &str);
    fn verifies(&self) -> bool;
}

pub(crate) const NOTHING: Tried = Tried {
    pasted: false,
    entered: false,
};

pub struct Refuse;

impl Courier for Refuse {
    fn facts(&self, _row: &Row, now_ms: i64) -> Facts {
        Facts {
            now_ms,
            job: Job::Missing { since_ms: now_ms },
            dialog_on_screen: false,
            queue_waiting: 0,
            safe: false,
            turn_over_since_ms: None,
        }
    }

    fn deliver(&self, _row: &Row, _guarded: bool) -> (TryEnd, Tried, Option<String>) {
        (TryEnd::AttachFailed, NOTHING, None)
    }

    fn close(&self, _session: &str) {}

    fn verifies(&self) -> bool {
        false
    }
}

pub(crate) trait JobFiles: Send + Sync {
    fn row(&self, job: &str) -> Option<Agent>;
    fn state(&self, job: &str) -> Option<String>;
    fn status(&self, job: &str) -> Option<String>;
    fn restarted(&self, agent: &Agent, now_ms: i64) -> bool;
    fn queue(&self, transcript: &str) -> Option<QueueState>;
    fn process(&self, job: &str) -> Option<(u32, Option<i64>)>;
    fn locate(&self, _agent: &Agent) -> Option<String> {
        None
    }
    fn transcript(&self, agent: &Agent) -> Option<String> {
        agent
            .transcript_path
            .clone()
            .filter(|path| Path::new(path).is_file())
            .or_else(|| self.locate(agent))
            .or_else(|| agent.transcript_path.clone())
    }
    fn born_ms(&self, path: &str) -> Option<i64> {
        made_ms(path)
    }
    fn shares_name(&self, _agent: &Agent, _path: &str) -> bool {
        false
    }
}

pub(crate) fn made_ms(path: &str) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let at = meta.created().or_else(|_| meta.modified()).ok()?;
    let ms = at.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis();
    i64::try_from(ms).ok()
}

pub(crate) struct LiveFiles {
    roots: Roots,
}

impl JobFiles for LiveFiles {
    fn row(&self, job: &str) -> Option<Agent> {
        fresh::job_row(&self.roots, &Probes::from_proc(), job)
    }

    fn state(&self, job: &str) -> Option<String> {
        fresh::job_state(&self.roots, job)
    }

    fn status(&self, job: &str) -> Option<String> {
        fresh::job_status(&self.roots, &Probes::from_proc(), job)
    }

    fn restarted(&self, agent: &Agent, now_ms: i64) -> bool {
        self.transcript(agent)
            .is_some_and(|path| restart::restarted(&path, now_ms))
    }

    fn queue(&self, transcript: &str) -> Option<QueueState> {
        queue_state(transcript)
    }

    fn process(&self, job: &str) -> Option<(u32, Option<i64>)> {
        fresh::job_process(&self.roots, &Probes::from_proc(), job)
    }

    fn locate(&self, agent: &Agent) -> Option<String> {
        let stem = agent
            .transcript_path
            .as_deref()
            .and_then(|path| Path::new(path).file_stem())
            .map(|stem| stem.to_string_lossy().into_owned());
        let ids: Vec<String> = agent
            .resume_session_id
            .clone()
            .into_iter()
            .chain(stem)
            .chain(Some(agent.id.clone()))
            .collect();
        let config = config_dirs(&self.roots)
            .into_iter()
            .find(|c| c.lane.as_ref().map(|l| l.path.as_str()) == agent.config_dir.as_deref())?;
        let cwds: Vec<&str> = [agent.worktree_path.as_deref(), Some(agent.cwd.as_str())]
            .into_iter()
            .flatten()
            .collect();
        find_in_folders(&config.dir.join("projects").to_string_lossy(), &ids, &cwds)
    }

    fn shares_name(&self, agent: &Agent, path: &str) -> bool {
        let AgentTarget::Job(short) = &agent.target else {
            return false;
        };
        let Some(config) = config_dirs(&self.roots)
            .into_iter()
            .find(|config| owns(&config.dir.join("projects"), Path::new(path)))
        else {
            return true;
        };
        let Some(folder) = Path::new(path)
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
        else {
            return true;
        };
        let projects = config.dir.join("projects").to_string_lossy().into_owned();
        let text =
            |value: &Value, key: &str| value.get(key).and_then(Value::as_str).map(String::from);
        let lands_here = |cwd: Option<String>, worktree: Option<String>, link: Option<String>| {
            [cwd, worktree]
                .into_iter()
                .flatten()
                .filter(|dir| !dir.is_empty())
                .any(|dir| project_folders(&dir).contains(&folder))
                || link
                    .as_deref()
                    .and_then(|link| Path::new(link).parent()?.file_name())
                    .is_some_and(|name| name.to_string_lossy() == folder.as_str())
        };
        let job = read_jobs(&config.dir).iter().any(|(id, job)| {
            id != short
                && lands_here(
                    text(job, "cwd"),
                    text(job, "worktreePath"),
                    text(job, "linkScanPath"),
                )
        });
        job || read_sessions(&config.dir, &|_| true).iter().any(|session| {
            text(session, "jobId").as_deref() != Some(short.as_str())
                && lands_here(
                    text(session, "cwd"),
                    text(session, "worktreePath"),
                    text(session, "sessionId")
                        .and_then(|id| find_transcript(&projects, &id, &text(session, "cwd")?)),
                )
        })
    }
}

fn owns(projects: &Path, path: &Path) -> bool {
    path.starts_with(projects)
        || match (projects.canonicalize(), path.canonicalize()) {
            (Ok(projects), Ok(path)) => path.starts_with(projects),
            _ => false,
        }
}

pub(crate) fn live_files(roots: Roots) -> Arc<dyn JobFiles> {
    Arc::new(LiveFiles { roots })
}

#[derive(Default)]
struct Memo {
    missing_since: Option<i64>,
    dialog_at: Option<i64>,
    calm_since: Option<i64>,
    last_seen: Option<i64>,
    answer: Option<AnswerWatch>,
}

pub struct ClaudeCourier {
    acting: Arc<ActingOps>,
    files: Arc<dyn JobFiles>,
    clock: Clock,
    attaches: Mutex<HashMap<String, Box<dyn Pty + Send>>>,
    memos: Mutex<HashMap<String, Memo>>,
    safe: Arc<SafeModes>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn asked_window(text: &str) -> (u64, u64) {
    match text.trim_start().strip_prefix("/model") {
        Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => {
            (MODEL_QUIET_MS, MODEL_WAIT_MS)
        }
        _ => (ASKED_QUIET_MS, ASKED_WAIT_MS),
    }
}

fn asks_something(pty: &mut dyn Pty, text: &str) -> bool {
    let (quiet, wait) = asked_window(text);
    pty.wait_quiet(Duration::from_millis(quiet), Duration::from_millis(wait));
    screen_dialog::read(&pty.screen()).is_some()
}

fn calm(files: &dyn JobFiles, fresh: &Agent, now_ms: i64) -> bool {
    fresh.takes_held_text()
        && (fresh.group != AgentGroup::Working || !files.restarted(fresh, now_ms))
}

impl ClaudeCourier {
    pub fn new(
        acting: Arc<ActingOps>,
        roots: Roots,
        clock: Clock,
        safe: Arc<SafeModes>,
    ) -> ClaudeCourier {
        ClaudeCourier::with_files(acting, live_files(roots), clock, safe)
    }

    pub(crate) fn with_files(
        acting: Arc<ActingOps>,
        files: Arc<dyn JobFiles>,
        clock: Clock,
        safe: Arc<SafeModes>,
    ) -> ClaudeCourier {
        ClaudeCourier {
            acting,
            files,
            clock,
            attaches: Mutex::new(HashMap::new()),
            memos: Mutex::new(HashMap::new()),
            safe,
        }
    }

    pub fn resume(&self, row: &Row) -> Result<(), ApiError> {
        self.acting
            .respawn(&row.session)
            .map(|_| ())
            .map_err(|e| ApiError::new(ErrorCode::Failed, e.to_string()))
    }

    #[cfg(test)]
    pub(crate) fn open(&self) -> usize {
        lock(&self.attaches).len()
    }

    fn answer_open(&self, row: &Row, job: &Job, fresh: Option<&Agent>, now_ms: i64) -> bool {
        let Job::Live { status } = job else {
            return true;
        };
        if now_ms - row.updated_ms < ANSWER_GRACE_MS {
            return true;
        }
        let taken = lock(&self.memos)
            .entry(row.session.clone())
            .or_default()
            .answer
            .take();
        let mut watch = taken.unwrap_or_default();
        let key = (row.id.as_str(), row.updated_ms);
        let open = watch.open(key, status.as_deref(), now_ms, || {
            let seen = match fresh {
                Some(agent) => self.acting.dialog_seen(agent),
                None => Seen::Unknown,
            };
            (seen, (self.clock)())
        });
        lock(&self.memos)
            .entry(row.session.clone())
            .or_default()
            .answer = open.then_some(watch);
        open
    }

    fn calm_now(&self, session: &str) -> bool {
        let now = (self.clock)();
        self.files
            .row(session)
            .is_some_and(|fresh| calm(self.files.as_ref(), &fresh, now))
    }

    fn job_of(&self, row: &Row, fresh: Option<&Agent>, now_ms: i64) -> (Job, bool) {
        let state = self.files.state(&row.session);
        match (fresh, state.as_deref()) {
            (_, Some("stopped")) => (Job::Stopped, false),
            (None, None) => (Job::Missing { since_ms: now_ms }, true),
            (Some(agent), _) if !agent.can_reply() => (
                Job::Missing {
                    since_ms: now_ms - GONE_AFTER_MS,
                },
                false,
            ),
            (Some(_), _) => (
                Job::Live {
                    status: self.files.status(&row.session),
                },
                false,
            ),
            (None, Some(_)) => (Job::Live { status: None }, false),
        }
    }

    fn try_once(&self, row: &Row, agent: &Agent, guarded: bool) -> (TryEnd, Tried, Option<String>) {
        let open = lock(&self.attaches).remove(&row.session);
        let (mut pty, plan) = match open {
            Some(pty) => (pty, plan::reply_in(&row.text)),
            None => match self.acting.attach(agent) {
                Ok(pty) => (pty, plan::reply(&row.text)),
                Err(err) => {
                    self.note(row, agent, Some(attach_why(&err)));
                    return (TryEnd::AttachFailed, NOTHING, None);
                }
            },
        };
        let began = (self.clock)();
        let clearing = Clearing {
            earlier: self.acting.attempts.recent(&agent.id, began),
            any: row.clear_box,
        };
        let (refused, pasted, entered) = (
            AtomicBool::new(false),
            AtomicBool::new(false),
            AtomicBool::new(false),
        );
        let guard = || !guarded || self.calm_now(&row.session);
        let mut trail = Vec::new();
        let played = {
            let mut wrapped = Guarded {
                inner: Box::new(&mut *pty),
                guard: &guard,
                refused: &refused,
                typed: &pasted,
                entered: Some(&entered),
                checked: false,
            };
            execute_clearing(
                &plan,
                &mut wrapped,
                self.acting.attacher.screen_check(),
                &mut NoLog,
                &mut trail,
                &clearing,
            )
        };
        let tried = Tried {
            pasted: pasted.load(Ordering::SeqCst),
            entered: entered.load(Ordering::SeqCst),
        };
        let mixed = matches!(played, Err(Failure::Mixed));
        if let (Some(text), true, true) =
            (plan.reply_text(), played.is_err(), tried.pasted || mixed)
        {
            self.acting.attempts.note(&agent.id, text, began);
        }
        if refused.load(Ordering::SeqCst) {
            self.note(row, agent, Some(FailWhy::NotIdle));
            return (TryEnd::NotIdle, tried, None);
        }
        match played {
            Ok(_) if row.kind == Kind::Command && asks_something(pty.as_mut(), &row.text) => {
                self.acting.attempts.forget(&agent.id);
                self.note(row, agent, None);
                (TryEnd::Asked, tried, None)
            }
            Ok(_) => {
                self.acting.attempts.forget(&agent.id);
                lock(&self.attaches).insert(row.session.clone(), pty);
                self.note(row, agent, None);
                (TryEnd::Ok, tried, None)
            }
            Err(failure) => {
                self.keep(row, &failure, pty.as_mut(), &trail);
                let note = self.note_of(&row.session, &failure, pty.as_mut());
                self.note(row, agent, Some(why_of(&failure)));
                (TryEnd::Failed(failure), tried, note)
            }
        }
    }

    fn note_of(&self, session: &str, failure: &Failure, pty: &mut dyn Pty) -> Option<String> {
        match failure {
            Failure::NotCleared | Failure::Mixed => {
                let lines = pty.screen();
                let (cursor, shades) = (pty.cursor(), pty.shades());
                input_box::typed_text_shaded(&lines, cursor, &shades).map(|text| shown_box(&text))
            }
            Failure::Dialog => {
                lock(&self.memos)
                    .entry(session.to_string())
                    .or_default()
                    .dialog_at = Some((self.clock)());
                None
            }
            Failure::Pty(_) => None,
            other => Some(other.to_string()),
        }
    }

    fn keep(&self, row: &Row, failure: &Failure, pty: &mut dyn Pty, trail: &[String]) {
        let Some(file) = &self.acting.reply_screen_file else {
            return;
        };
        if matches!(failure, Failure::Pty(_)) {
            return;
        }
        let chars = row.text.chars().count();
        let head = crate::reply_screen::head(&row.session, failure, chars, pty.cursor(), trail);
        keep_screen(file, &head, &crate::reply_screen::masked(&pty.screen()));
    }

    fn note(&self, row: &Row, agent: &Agent, failure: Option<FailWhy>) {
        let waited = row
            .attempt_ms
            .map(|at| at.saturating_sub(row.queued_ms))
            .and_then(|ms| u64::try_from(ms).ok());
        self.acting.note_reply(
            session_ref(agent),
            reply_via(row.via),
            row.text.chars().count(),
            failure,
            waited,
            false,
        );
    }
}

impl Courier for ClaudeCourier {
    fn facts(&self, row: &Row, now_ms: i64) -> Facts {
        let fresh = self.files.row(&row.session);
        let (job, missing) = self.job_of(row, fresh.as_ref(), now_ms);
        let queue = fresh
            .as_ref()
            .and_then(|agent| self.files.transcript(agent))
            .and_then(|path| self.files.queue(&path))
            .unwrap_or_default();
        let safe = self.safe.is_safe(&row.session, &queue);
        let reading = (row.kind == Kind::Command || safe || !queue.waiting.is_empty()).then(|| {
            fresh
                .as_ref()
                .is_some_and(|agent| calm(self.files.as_ref(), agent, now_ms))
        });
        let mut memos = lock(&self.memos);
        let memo = memos.entry(row.session.clone()).or_default();
        let job = match job {
            Job::Missing { .. } if missing => Job::Missing {
                since_ms: *memo.missing_since.get_or_insert(now_ms),
            },
            other => {
                memo.missing_since = None;
                other
            }
        };
        let gap = memo
            .last_seen
            .replace(now_ms)
            .is_some_and(|at| now_ms - at > CALM_GAP_MS);
        if gap {
            memo.calm_since = None;
        }
        let turn_over_since_ms = match reading {
            Some(true) => Some(*memo.calm_since.get_or_insert(now_ms)),
            _ => {
                memo.calm_since = None;
                None
            }
        };
        let remembered = memo
            .dialog_at
            .is_some_and(|at| now_ms - at < DIALOG_RECHECK_MS);
        drop(memos);
        let dialog_on_screen = match row.why {
            Some(Why::Answer) => self.answer_open(row, &job, fresh.as_ref(), now_ms),
            _ => remembered,
        };
        Facts {
            now_ms,
            job,
            dialog_on_screen,
            queue_waiting: queue.waiting.len(),
            safe,
            turn_over_since_ms,
        }
    }

    fn deliver(&self, row: &Row, guarded: bool) -> (TryEnd, Tried, Option<String>) {
        let Some(agent) = self.files.row(&row.session).filter(Agent::can_reply) else {
            self.close(&row.session);
            return (TryEnd::AttachFailed, NOTHING, None);
        };
        let typing = self.acting.typing_locks().for_job(&row.session);
        let _held = lock(&typing);
        self.try_once(row, &agent, guarded)
    }

    fn close(&self, session: &str) {
        let open = lock(&self.attaches).remove(session);
        if let Some(mut pty) = open {
            let detach = plan::Plan {
                steps: Vec::new(),
                detach: plan::reply("").detach,
            };
            let _ = execute_clearing(
                &detach,
                pty.as_mut(),
                self.acting.attacher.screen_check(),
                &mut NoLog,
                &mut Vec::new(),
                &Clearing::default(),
            );
        }
    }

    fn verifies(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[path = "outbox_courier_tests.rs"]
mod tests;
