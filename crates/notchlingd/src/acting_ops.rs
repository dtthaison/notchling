//! Milestone 3's ops: acting on sessions (E3b, E4), the `acting` switch, the held prompts and the
//! hook's request. Built and switched off: while [`Acting`] is off every acting op answers
//! `forbidden` with [`ACTING_OFF`], nothing is typed, and `hook.request` passes.
//!
//! | Op | Tier | Args | Reply |
//! | --- | --- | --- | --- |
//! | `session.reply` | app | `{id, text}` | `{"sent": true}` |
//! | `session.answer` | app | `{id, answers: [[n...] or "text"], question?: {items}}` | `{"answered": "hook" or "terminal", "promptId"?}` |
//! | `session.permission` | app | `{id, decision: "allow"/"deny"/"always", message?}` | `{"decided": decision, "promptId"}` |
//! | `session.interrupt` | app | `{id}` | `{"sent": true}` |
//! | `session.take_back` | app | `{id}` | `{"taken": bool}` |
//! | `session.stop`, `.remove`, `.new`, `.move`, `.command`, `folder.trust` | app | see [`crate::control_ops`] | |
//! | `settings.set_acting` | app | `{on: bool}` | `{"acting": bool}` |
//! | `prompts.list` | app | `{}` | `[prompt...]` |
//! | `settings.get_acting` | agent | `{}` | `{"acting": bool}` |
//! | `session.question` | agent | `{id}` | `{"promptId", "items"}`, or `unimplemented` for one only on screen (`ReplySender.readQuestion` isn't ported) |
//! | `hook.request` | agent, `client: "hook"` only, from a process a Claude Code session started | `{event, waitMs?}` | a decision (`prompts`) |
//!
//! An `id` is a row id from `sessions.list`. A held prompt is answered by its own id from
//! `prompt.pending` only (`session.permission` always; `session.answer` when the id is a prompt's,
//! else it types into the row's dialog), never found from a row: a prompt is the one the app showed.
//! A typing failure (a dialog open, the question gone) is code `failed`.
//!
//! The hook is who its process is, never what it says: see [`crate::hook_ops`].

use crate::acting::{Acting, ACTING_OFF};
use crate::agy_ops::AgyOps;
use crate::hook_ops::Parents;
use crate::prompts::{question_json, Kind, Prompt, PromptStore};
use notchling_engine::control::{Command, LaunchError, LaunchOutput, Launcher, PtyScript};
use notchling_engine::reply::exec::{execute, Outcome, Pty, QueueLog, ScreenCheck, TranscriptLog};
use notchling_engine::reply::plan::{self, Plan, PlanError};
use notchling_engine::reply::question::{
    choices_from_json, hook_answers, PendingQuestion, QuestionItem,
};
use notchling_engine::sessions::jobs::config_dir_for_job;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget, Roots};
use notchling_local_api::{ops, topics, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The longest a hook is held (the Swift hook's `waitSeconds`), under Claude Code's 300 s timeout.
pub const HOOK_WAIT: Duration = Duration::from_secs(290);

/// After an Esc, a session that still shows working is stopping for this long, and a second Esc
/// isn't sent (`ChatModel.isStopping`): on a session that went idle meanwhile it would open rewind.
pub const STOPPING_FOR: Duration = Duration::from_secs(20);

/// The ops that act on a session, refused while acting is off.
pub const ACTING_OPS: [&str; 11] = [
    ops::SESSION_REPLY,
    ops::SESSION_ANSWER,
    ops::SESSION_PERMISSION,
    ops::SESSION_INTERRUPT,
    ops::SESSION_STOP,
    ops::SESSION_REMOVE,
    ops::SESSION_TAKE_BACK,
    ops::SESSION_NEW,
    ops::FOLDER_TRUST,
    ops::SESSION_MOVE,
    ops::SESSION_COMMAND,
];

/// Session control (E11, `engine::control`), run by [`crate::control_ops`].
pub const CONTROL_OPS: [&str; 6] = [
    ops::SESSION_STOP,
    ops::SESSION_REMOVE,
    ops::SESSION_NEW,
    ops::FOLDER_TRUST,
    ops::SESSION_MOVE,
    ops::SESSION_COMMAND,
];

/// The topics these ops publish on.
pub const TOPICS: [&str; 2] = [topics::PROMPTS, topics::SETTINGS];

/// Where events go, and whether anyone who can answer is there.
pub trait Room: Send + Sync {
    fn publish(&self, topic: &str, event: &str, data: &Value);
    /// An app-tier client is attached (later a paired phone too).
    fn can_answer(&self) -> bool;
    /// An app-tier client is attached and follows `topic`, so an event published there is seen.
    /// Attached alone says nothing about what the client reads: the pairing request needs this,
    /// not [`Room::can_answer`].
    fn can_follow(&self, topic: &str) -> bool;
}

/// Opens the attach terminal of a background job: `claude attach <job>` in a pty of
/// [`notchling_engine::reply::keys::timing`]'s size, `TERM=xterm-256color`, `NO_COLOR` dropped,
/// `CLAUDE_CONFIG_DIR` set to `config_dir` for a lane's job.
pub trait Attacher: Send + Sync {
    fn attach(&self, job: &str, config_dir: Option<&str>) -> Result<Box<dyn Pty>, ApiError>;
    /// What reads the screen (E3a's `AskDialog`).
    fn screen_check(&self) -> &dyn ScreenCheck;
}

/// An attacher that opens nothing, for tests that must not type (the daemon uses
/// [`crate::attach::ClaudeAttacher`]).
pub struct NoTerminal;

impl Attacher for NoTerminal {
    fn attach(&self, _job: &str, _config_dir: Option<&str>) -> Result<Box<dyn Pty>, ApiError> {
        Err(unimplemented("this daemon types into no session"))
    }
    fn screen_check(&self) -> &dyn ScreenCheck {
        &notchling_engine::reply::exec::Markers
    }
}

fn unimplemented(message: &str) -> ApiError {
    ApiError::new(ErrorCode::Unimplemented, message)
}

pub(crate) fn forbidden(message: &str) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, message)
}

pub(crate) fn bad(message: impl Into<String>) -> ApiError {
    ApiError::bad_request(message)
}

/// Waits this many milliseconds (session control's pauses).
pub type Pause = Box<dyn Fn(u64) + Send + Sync>;

/// The launcher until [`ActingOps::with_launcher`] sets one: nothing starts.
struct NoLaunch;

impl Launcher for NoLaunch {
    fn run(&self, _command: &Command, _timeout: Duration) -> Result<LaunchOutput, LaunchError> {
        Err(LaunchError::Unsupported)
    }
    fn run_pty(&self, _script: &PtyScript) -> Result<bool, LaunchError> {
        Err(LaunchError::Unsupported)
    }
}

/// The ops, with what they need.
pub struct ActingOps {
    pub(crate) acting: Arc<Acting>,
    pub(crate) prompts: PromptStore,
    sessions: Arc<Mutex<Value>>,
    pub(crate) roots: Roots,
    pub(crate) room: Arc<dyn Room>,
    attacher: Box<dyn Attacher>,
    /// Starts `claude stop`, `rm`, `--bg` and the trust pty (session control).
    pub(crate) launcher: Arc<dyn Launcher>,
    pub(crate) pause: Pause,
    /// One attach at a time, so a question read can't move the tabs under an answer.
    typing: Mutex<()>,
    /// How often a held hook looks at the app and its own process.
    pub(crate) tick: Duration,
    /// Walks a hook's process up to its Claude Code.
    pub(crate) parent_of: Parents,
    /// Sessions sent an Esc, and when (by row id).
    stopping: Mutex<HashMap<String, Instant>>,
    /// Folders `session.new` was refused as untrusted, canonical, and when: `folder.trust` takes
    /// only these ([`crate::control_ops`]).
    pub(crate) refused: Mutex<HashMap<PathBuf, Instant>>,
    /// Antigravity sessions (Milestone 5): what turning acting off must close, and whose waiting
    /// permissions `prompts.list` shows.
    agy: OnceLock<Arc<AgyOps>>,
}

impl ActingOps {
    pub fn new(
        acting: Arc<Acting>,
        sessions: Arc<Mutex<Value>>,
        roots: Roots,
        room: Arc<dyn Room>,
        attacher: Box<dyn Attacher>,
    ) -> ActingOps {
        ActingOps {
            acting,
            prompts: PromptStore::default(),
            sessions,
            roots,
            room,
            attacher,
            launcher: Arc::new(NoLaunch),
            pause: Box::new(|ms| std::thread::sleep(Duration::from_millis(ms))),
            typing: Mutex::new(()),
            tick: Duration::from_millis(250),
            parent_of: Box::new(crate::hook::parent_pid),
            stopping: Mutex::new(HashMap::new()),
            refused: Mutex::new(HashMap::new()),
            agy: OnceLock::new(),
        }
    }

    /// The switch, shared with the ops of Milestone 5 that act on their own sessions.
    pub fn acting_switch(&self) -> Arc<Acting> {
        self.acting.clone()
    }

    /// The Antigravity ops, so acting off closes their sessions and `prompts.list` includes
    /// their permissions. Once.
    pub fn attach_agy(&self, agy: Arc<AgyOps>) {
        let _ = self.agy.set(agy);
    }

    /// Moves every stopping mark `by` into the past (tests).
    #[cfg(test)]
    pub(crate) fn age_stopping(&self, by: Duration) {
        age(&self.stopping, by);
    }

    /// Moves every recorded refusal `by` into the past (tests).
    #[cfg(test)]
    pub(crate) fn age_refusals(&self, by: Duration) {
        age(&self.refused, by);
    }

    /// Session control runs through `launcher`, and waits with `pause`.
    pub fn with_launcher(mut self, launcher: Arc<dyn Launcher>, pause: Pause) -> ActingOps {
        self.launcher = launcher;
        self.pause = pause;
        self
    }

    /// The switch, for `daemon.status`.
    pub fn is_on(&self) -> bool {
        self.acting.is_on()
    }

    pub fn prompts(&self) -> &PromptStore {
        &self.prompts
    }

    /// The daemon is stopping: every held hook passes.
    pub fn close(&self) {
        for prompt in self.prompts.close() {
            self.resolved(&prompt, "pass", "stopping");
        }
    }

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::SETTINGS_SET_ACTING | ops::PROMPTS_LIST => Some(OpSpec::APP),
            ops::SETTINGS_GET_ACTING | ops::SESSION_QUESTION | ops::HOOK_REQUEST => {
                Some(OpSpec::AGENT)
            }
            op if ACTING_OPS.contains(&op) => Some(OpSpec::APP),
            _ => None,
        }
    }

    pub fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if ACTING_OPS.contains(&op) && !self.acting.is_on() {
            return Err(forbidden(ACTING_OFF));
        }
        match op {
            ops::SETTINGS_GET_ACTING => Ok(json!({"acting": self.acting.is_on()})),
            ops::SETTINGS_SET_ACTING => self.set_acting(args),
            ops::PROMPTS_LIST => {
                let mut list: Vec<Value> =
                    self.prompts.list().iter().map(Prompt::to_json).collect();
                if let Some(agy) = self.agy.get() {
                    list.extend(agy.prompts());
                }
                Ok(Value::Array(list))
            }
            ops::SESSION_QUESTION => self.question(args),
            ops::HOOK_REQUEST => self.hook_request(caller, args),
            ops::SESSION_REPLY => self.reply(args),
            ops::SESSION_INTERRUPT => self.interrupt(args),
            ops::SESSION_TAKE_BACK => self.take_back(args),
            ops::SESSION_PERMISSION => self.permission(args),
            ops::SESSION_ANSWER => self.answer(args),
            op if CONTROL_OPS.contains(&op) => self.control(op, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn set_acting(&self, args: &Value) -> Result<Value, ApiError> {
        let on = args
            .get("on")
            .and_then(Value::as_bool)
            .ok_or_else(|| bad("settings.set_acting takes {\"on\": true|false}"))?;
        self.acting.set(on);
        if !on {
            // Every held hook passes now, not at its deadline: nobody may decide them any more.
            for prompt in self.prompts.drain() {
                self.resolved(&prompt, "pass", "acting_off");
            }
            // No `agy` stays running while the Swift app owns the sessions.
            if let Some(agy) = self.agy.get() {
                agy.acting_off();
            }
        }
        let data = json!({"acting": on});
        self.room
            .publish(topics::SETTINGS, topics::ACTING_CHANGED, &data);
        Ok(data)
    }

    /// The row `args.id` names in the current session list.
    pub(crate) fn agent(&self, args: &Value) -> Result<Agent, ApiError> {
        let id = text_arg(args, "id")?;
        let list = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let row = list
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r.get("id") == Some(&json!(id))))
            .ok_or_else(|| bad(format!("no session {id}")))?;
        serde_json::from_value(row.clone()).map_err(|e| ApiError::internal(e.to_string()))
    }

    /// Plays `plan` into the job's attach terminal, one attach at a time.
    pub(crate) fn play(
        &self,
        agent: &Agent,
        plan: &Plan,
        log: &mut dyn QueueLog,
    ) -> Result<Outcome, ApiError> {
        let _one = self.typing.lock().unwrap_or_else(|e| e.into_inner());
        self.play_locked(agent, plan, log)
    }

    /// [`ActingOps::play`] for a caller that holds `typing` already.
    fn play_locked(
        &self,
        agent: &Agent,
        plan: &Plan,
        log: &mut dyn QueueLog,
    ) -> Result<Outcome, ApiError> {
        if plan.is_empty() {
            return Ok(Outcome::default());
        }
        let job = match &agent.target {
            AgentTarget::Job(job) if agent.can_reply() => job.clone(),
            _ => return Err(bad("this session takes no replies")),
        };
        // A lane's job is reachable only through its own config folder.
        let config_dir = config_dir_for_job(&self.roots, &job).or_else(|| agent.config_dir.clone());
        let mut pty = self.attacher.attach(&job, config_dir.as_deref())?;
        execute(plan, pty.as_mut(), self.attacher.screen_check(), log)
            .map_err(|f| ApiError::new(ErrorCode::Failed, f.to_string()))
    }

    fn reply(&self, args: &Value) -> Result<Value, ApiError> {
        let text = text_arg(args, "text")?;
        if text.trim().is_empty() {
            return Err(bad("the reply is empty"));
        }
        let agent = self.agent(args)?;
        self.play(&agent, &plan::reply(text), &mut NoLog)?;
        Ok(json!({"sent": true}))
    }

    /// Esc for a working session, once: the row and its stopping mark are read under the typing
    /// lock, so two interrupts can't both see it working and both send Esc, and a second one within
    /// [`STOPPING_FOR`] of the first is a no-op (`{"sent": false, "stopping": true}`).
    fn interrupt(&self, args: &Value) -> Result<Value, ApiError> {
        let id = text_arg(args, "id")?.to_string();
        let _one = self.typing.lock().unwrap_or_else(|e| e.into_inner());
        let agent = self.agent(args)?;
        let working = agent.group == AgentGroup::Working;
        {
            let mut marks = self.stopping.lock().unwrap_or_else(|e| e.into_inner());
            marks.retain(|_, at| at.elapsed() < STOPPING_FOR);
            if working && marks.contains_key(&id) {
                return Ok(json!({"sent": false, "stopping": true}));
            }
        }
        let plan = plan::interrupt(working).map_err(|e| match e {
            PlanError::NotWorking => bad(
                "the session isn't working, so there's nothing to stop (Esc on an idle session opens rewind)",
            ),
        })?;
        self.play_locked(&agent, &plan, &mut NoLog)?;
        self.stopping
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Instant::now());
        Ok(json!({"sent": true}))
    }

    fn take_back(&self, args: &Value) -> Result<Value, ApiError> {
        let agent = self.agent(args)?;
        let transcript = agent
            .transcript_path
            .clone()
            .ok_or_else(|| bad("this session has no transcript to read its queue from"))?;
        let mut queued = notchling_engine::transcript::conversation::read(&transcript).queued;
        let latest = queued.pop().ok_or_else(|| bad("nothing is queued"))?;
        let mut log = TranscriptLog::new(&transcript);
        let outcome = self.play(&agent, &plan::take_back(&latest, &queued), &mut log)?;
        Ok(json!({"taken": outcome.taken}))
    }

    /// The held prompt `args.id` names by its own id. Never found from a session row: an answer
    /// goes to the prompt the app showed, not to whichever one a session has now.
    fn held(&self, args: &Value) -> Result<Option<Prompt>, ApiError> {
        Ok(self.prompts.get(text_arg(args, "id")?))
    }

    /// For reading only (`session.question`): a prompt id, or the row's oldest held question.
    fn held_for_reading(&self, args: &Value) -> Result<Option<Prompt>, ApiError> {
        if let Some(prompt) = self.held(args)? {
            return Ok(Some(prompt));
        }
        let Ok(agent) = self.agent(args) else {
            return Ok(None);
        };
        Ok(agent
            .session_id()
            .and_then(|s| self.prompts.for_session(&s, Kind::Question)))
    }

    fn permission(&self, args: &Value) -> Result<Value, ApiError> {
        let decision = text_arg(args, "decision")?;
        let prompt = self
            .held(args)?
            .ok_or_else(|| bad("no prompt with that id is waiting (answer by the prompt's id)"))?;
        let response = match (decision, prompt.kind) {
            ("deny", _) => match args.get("message").and_then(Value::as_str) {
                Some(message) => json!({"action": "deny", "message": message}),
                None => json!({"action": "deny"}),
            },
            ("allow", Kind::Permission) => json!({"action": "allow"}),
            ("always", Kind::Permission) => json!({"action": "allow", "always": true}),
            ("allow" | "always", Kind::Question) => {
                return Err(bad("a question is answered with session.answer"))
            }
            _ => return Err(bad("decision is allow, deny or always")),
        };
        self.decide(&prompt, response, decision)?;
        Ok(json!({"decided": decision, "promptId": prompt.id}))
    }

    fn answer(&self, args: &Value) -> Result<Value, ApiError> {
        let answers = args
            .get("answers")
            .ok_or_else(|| bad("answers is needed"))?;
        if let Some(prompt) = self.held(args)? {
            let question = prompt
                .question
                .clone()
                .ok_or_else(|| bad("that prompt isn't a question"))?;
            let choices = choices_from_json(answers, &question.items).map_err(bad)?;
            let response =
                json!({"action": "answer", "answers": hook_answers(&choices, &question)});
            self.decide(&prompt, response, "answer")?;
            return Ok(json!({"answered": "hook", "promptId": prompt.id}));
        }
        // Not held by a hook: typed into the job's dialog, checked page by page.
        let agent = self.agent(args)?;
        let question = question_arg(args)?;
        let choices = choices_from_json(answers, &question.items).map_err(bad)?;
        self.play(&agent, &plan::answer(&choices, &question), &mut NoLog)?;
        Ok(json!({"answered": "terminal"}))
    }

    fn decide(&self, prompt: &Prompt, response: Value, outcome: &str) -> Result<(), ApiError> {
        let resolved = self
            .prompts
            .resolve(&prompt.id, response)
            .ok_or_else(|| bad("that prompt was already answered or given up"))?;
        self.resolved(&resolved, outcome, "app");
        Ok(())
    }

    pub(crate) fn resolved(&self, prompt: &Prompt, outcome: &str, by: &str) {
        let data =
            json!({"id": prompt.id, "sessionId": prompt.session_id, "outcome": outcome, "by": by});
        self.room
            .publish(topics::PROMPTS, topics::PROMPT_RESOLVED, &data);
    }

    fn question(&self, args: &Value) -> Result<Value, ApiError> {
        if let Some(prompt) = self.held_for_reading(args)? {
            if let Some(q) = &prompt.question {
                let mut out = question_json(q);
                out["promptId"] = json!(prompt.id);
                return Ok(out);
            }
        }
        // Reading the screen moves the dialog's tabs: that's acting.
        if !self.acting.is_on() {
            return Err(forbidden(ACTING_OFF));
        }
        Err(unimplemented(
            "reading a question off the screen isn't ported yet (ReplySender.readQuestion)",
        ))
    }
}

pub(crate) struct NoLog;

impl QueueLog for NoLog {
    fn mark(&mut self) {}
    fn popped_all(&mut self, _text: &str) -> bool {
        false
    }
}

pub(crate) fn text_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("{key} is needed")))
}

/// `args.question`: `{"items": [{header?, question, options, details?, multiSelect?}]}`.
fn question_arg(args: &Value) -> Result<PendingQuestion, ApiError> {
    let items = args
        .get("question")
        .and_then(|q| q.get("items"))
        .and_then(Value::as_array)
        .ok_or_else(|| bad("no question is held for this session; pass the question read with session.question"))?;
    let strings = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    };
    let items = items
        .iter()
        .map(|i| QuestionItem {
            header: i.get("header").and_then(Value::as_str).unwrap_or("").into(),
            question: i
                .get("question")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            options: strings(i.get("options")),
            details: strings(i.get("details")),
            multi_select: i
                .get("multiSelect")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect();
    Ok(PendingQuestion {
        tool_use_id: "screen".into(),
        items,
    })
}

#[cfg(test)]
fn age<K>(marks: &Mutex<HashMap<K, Instant>>, by: Duration) {
    for at in marks.lock().unwrap_or_else(|e| e.into_inner()).values_mut() {
        *at = at.checked_sub(by).unwrap_or(*at);
    }
}

pub(crate) fn now_ms() -> i64 {
    notchling_engine::reply::popall::now_ms()
}
