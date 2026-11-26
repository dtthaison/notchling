use crate::op_group::OpGroup;
use notchling_engine::sessions::{config_dirs, Roots};
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::locate::find_transcript;
use notchling_engine::transcript::model::{BackgroundShell, Conversation};
use notchling_engine::transcript::shells::{self, ShellState};
use notchling_engine::transcript::shells_guard;
use notchling_engine::transcript::shells_progress::Tracker;
use notchling_engine::transcript::shells_stop::{self, Outcome, Refusal, StopRequest, GRACE};
use notchling_engine::transcript::shells_stuck;
use notchling_local_api::{
    bound_identity, ops, AgentIdentity, ApiError, Caller, ErrorCode, OpSpec,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

pub const SOME_LEFT: &str = "some_left";
pub const NO_PROGRESS: &str = "no_progress";

pub type Writers = Arc<dyn Fn(&[String]) -> HashMap<String, Vec<i32>> + Send + Sync>;
pub type Stopper = Arc<dyn Fn(&StopRequest, Duration) -> Result<Outcome, Refusal> + Send + Sync>;
pub type Owner = Arc<dyn Fn(&Caller, &str) -> Option<u32> + Send + Sync>;
pub type Now = Arc<dyn Fn() -> f64 + Send + Sync>;
pub type Who = Arc<dyn Fn(&Caller) -> AgentIdentity + Send + Sync>;
pub type SessionOwner = Arc<dyn Fn(&str) -> Option<u32> + Send + Sync>;
pub type Progress = Arc<Mutex<Tracker>>;

pub fn progress_lock(progress: &Progress) -> MutexGuard<'_, Tracker> {
    progress.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct ShellsOps {
    roots: Roots,
    writers: Writers,
    stopper: Stopper,
    now: Now,
    who: Option<Who>,
    owner: Owner,
    session_owner: SessionOwner,
    progress: Progress,
}

fn clock() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn forbidden_no_session() -> ApiError {
    ApiError::new(
        ErrorCode::Forbidden,
        "Notchling can't tell which session this is.",
    )
    .with_reason("no_session")
}

fn refused(refusal: Refusal) -> ApiError {
    let words = match refusal {
        Refusal::NotAShellOutput => "That isn't this shell's own output file.",
        Refusal::OwnerUnknown => {
            "Notchling can't tell which Claude Code process owns this session."
        }
    };
    ApiError::new(ErrorCode::Forbidden, words).with_reason(refusal.reason())
}

fn not_here() -> ApiError {
    ApiError::bad_request("No shell with that id in this session.").with_reason("other_session")
}

impl ShellsOps {
    pub fn new(roots: Roots) -> ShellsOps {
        let found = roots.clone();
        let by_table = roots.clone();
        ShellsOps {
            roots,
            writers: Arc::new(shells_stop::writers_of_many),
            stopper: Arc::new(shells_stop::stop_shell),
            now: Arc::new(clock),
            who: None,
            owner: Arc::new(move |_, session| {
                shells_stop::process_table()
                    .and_then(|table| shells_guard::session_owner(&found, session, &table))
            }),
            session_owner: Arc::new(move |session| {
                shells_stop::process_table()
                    .and_then(|table| shells_guard::session_owner(&by_table, session, &table))
            }),
            progress: Arc::new(Mutex::new(Tracker::default())),
        }
    }

    pub fn with_progress(mut self, progress: Progress) -> ShellsOps {
        self.progress = progress;
        self
    }

    pub fn with_session_owner(mut self, owner: SessionOwner) -> ShellsOps {
        self.session_owner = owner;
        self
    }

    pub(crate) fn progress(&self) -> &Progress {
        &self.progress
    }

    pub(crate) fn writers_of(&self, paths: &[String]) -> HashMap<String, Vec<i32>> {
        (self.writers)(paths)
    }

    pub(crate) fn clock_s(&self) -> f64 {
        (self.now)()
    }

    pub fn with_owner(mut self, owner: Owner) -> ShellsOps {
        self.owner = owner;
        self
    }

    pub fn with_seams(
        mut self,
        writers: Writers,
        stopper: Stopper,
        now: Now,
        who: Option<Who>,
    ) -> ShellsOps {
        self.writers = writers;
        self.stopper = stopper;
        self.now = now;
        self.who = who;
        self
    }

    fn session_of(&self, caller: &Caller) -> Result<(String, Option<String>), ApiError> {
        let who = bound_identity()
            .or_else(|| self.who.as_ref().map(|who| who(caller)))
            .unwrap_or_else(AgentIdentity::shell);
        match (who.verified, who.session) {
            (true, Some(session)) => Ok((session, who.folder)),
            _ => Err(forbidden_no_session()),
        }
    }

    fn conversation_of(&self, session: &str, folder: Option<&str>) -> Option<Conversation> {
        config_dirs(&self.roots)
            .iter()
            .find_map(|lane| {
                let projects = lane.dir.join("projects");
                find_transcript(
                    &projects.to_string_lossy(),
                    session,
                    folder.unwrap_or_default(),
                )
            })
            .map(|path| conversation::read(&path))
    }

    fn row(
        &self,
        shell: &BackgroundShell,
        c: &Conversation,
        writers: &HashMap<String, Vec<i32>>,
    ) -> Value {
        let state = shells::state(shell, &c.finished_agents, &|p| {
            writers.get(p).cloned().unwrap_or_default()
        });
        let (running, status) = match &state {
            ShellState::Running => (true, "running".to_string()),
            ShellState::Ended(how) => (false, how.clone()),
        };
        let now = (self.now)();
        let secs = running
            .then(|| shells_stuck::running_secs(shell.started, now))
            .flatten();
        let stuck = running
            .then(|| shells_stuck::looks_stuck(&shell.command, secs))
            .flatten();
        let quiet = running
            .then(|| progress_lock(&self.progress).verdict(&shell.output_path, now as u64, secs))
            .flatten();
        let reason = match (&quiet, stuck) {
            (Some(_), _) => Some(NO_PROGRESS),
            (None, stuck) => stuck.map(|s| s.code()),
        };
        json!({
            "id": shell.id,
            "command": shell.command,
            "title": shell.title,
            "description": shell.description,
            "state": if running { "running" } else { "ended" },
            "status": status,
            "runningSecs": secs,
            "stuck": reason.is_some(),
            "stuckReason": reason,
            "quietMin": quiet.map(|q| q.minutes()),
            "output": shell.output_path,
        })
    }

    fn mine(&self, caller: &Caller) -> Result<Value, ApiError> {
        let (session, folder) = self.session_of(caller)?;
        let Some(c) = self.conversation_of(&session, folder.as_deref()) else {
            return Ok(json!({"shells": []}));
        };
        let mut paths: Vec<String> = c
            .shells
            .iter()
            .filter(|s| !c.finished_agents.contains_key(&s.id) && !s.output_path.is_empty())
            .map(|s| s.output_path.clone())
            .collect();
        paths.sort();
        paths.dedup();
        let writers = if paths.is_empty() {
            HashMap::new()
        } else {
            (self.writers)(&paths)
        };
        let rows: Vec<Value> = c
            .shells
            .iter()
            .rev()
            .map(|s| self.row(s, &c, &writers))
            .collect();
        Ok(json!({"shells": rows}))
    }

    fn stuck(&self, caller: &Caller) -> Result<Value, ApiError> {
        let mut rows = self.mine(caller)?;
        if let Some(list) = rows.get_mut("shells").and_then(Value::as_array_mut) {
            list.retain(|row| row.get("stuck").and_then(Value::as_bool) == Some(true));
        }
        Ok(rows)
    }

    fn stop(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| ApiError::bad_request("shells.stop takes {\"id\": \"<shell id>\"}"))?;
        let (session, folder) = self.session_of(caller)?;
        let c = self
            .conversation_of(&session, folder.as_deref())
            .ok_or_else(not_here)?;
        let owner = (self.owner)(caller, &session);
        self.stop_in(&c, &session, id, owner)
    }

    pub(crate) fn stop_for_inbox(
        &self,
        c: &Conversation,
        session: &str,
        id: &str,
    ) -> Result<Value, ApiError> {
        let owner = (self.session_owner)(session);
        self.stop_in(c, session, id, owner)
    }

    fn stop_in(
        &self,
        c: &Conversation,
        session: &str,
        id: &str,
        owner: Option<u32>,
    ) -> Result<Value, ApiError> {
        let shell = c.shells.iter().find(|s| s.id == id).ok_or_else(not_here)?;
        if c.finished_agents.contains_key(&shell.id) || shell.output_path.is_empty() {
            return Ok(json!({"id": id, "stopped": false, "state": "ended"}));
        }
        let request = StopRequest {
            session,
            id: &shell.id,
            path: &shell.output_path,
            owner,
            orphans: false,
        };
        let outcome = (self.stopper)(&request, GRACE).map_err(refused)?;
        if !outcome.left.is_empty() {
            return Ok(json!({
                "id": id,
                "stopped": false,
                "state": "running",
                "killed": outcome.killed,
                "left": outcome.left,
                "reason": SOME_LEFT,
            }));
        }
        if outcome.survivors > 0 {
            return Err(ApiError::new(
                ErrorCode::Failed,
                "The shell's processes didn't end.",
            ));
        }
        Ok(json!({
            "id": id,
            "stopped": outcome.targets > 0,
            "state": "ended",
            "killed": outcome.killed,
        }))
    }
}

impl OpGroup for ShellsOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::SHELLS_MINE | ops::SHELLS_STUCK => Some(OpSpec::AGENT_READ),
            ops::SHELLS_STOP => Some(OpSpec::AGENT_ACTS),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::SHELLS_MINE => self.mine(caller),
            ops::SHELLS_STUCK => self.stuck(caller),
            ops::SHELLS_STOP => self.stop(caller, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

#[cfg(test)]
#[path = "shells_ops_tests.rs"]
mod tests;
