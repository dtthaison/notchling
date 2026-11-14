use crate::acting::{Acting, ACTING_OFF};
use crate::acting_events::{attach_why, log_down, session_ref, who, why_of, PlayError, Who};
use crate::agy_ops::AgyOps;
use crate::catalog_ops::CatalogService;
use crate::codex_ops::CodexOps;
use crate::hook_ops::Parents;
use crate::late_emit::Events;
use crate::prompts::{fit_list, question_json, Kind, Prompt, PromptStore};
use notchling_engine::control::{Command, LaunchError, LaunchOutput, Launcher, PtyScript};
use notchling_engine::reply::exec::{
    execute, Failure, Outcome, Pty, QueueLog, ScreenCheck, TranscriptLog,
};
use notchling_engine::reply::plan::{self, Decision, Expected, Plan, PlanError};
use notchling_engine::reply::question::{
    choices_from_json, hook_answers, PendingQuestion, QuestionItem,
};
use notchling_engine::reply::read::read_question;
use notchling_engine::sessions::jobs::config_dir_for_job;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget, Roots};
use notchling_events::kinds::agents::{AnswerKind, HeldThen, PromptKind};
use notchling_events::kinds::choices::DoneOrFailed;
use notchling_events::{Ref, Session};
use notchling_local_api::{ops, topics, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const HOOK_WAIT: Duration = Duration::from_secs(290);

pub const STOPPING_FOR: Duration = Duration::from_secs(20);

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

pub const CONTROL_OPS: [&str; 6] = [
    ops::SESSION_STOP,
    ops::SESSION_REMOVE,
    ops::SESSION_NEW,
    ops::FOLDER_TRUST,
    ops::SESSION_MOVE,
    ops::SESSION_COMMAND,
];

pub const TOPICS: [&str; 2] = [topics::PROMPTS, topics::SETTINGS];

pub trait Room: Send + Sync {
    fn publish(&self, topic: &str, event: &str, data: &Value);
    fn can_answer(&self) -> bool;
    fn can_follow(&self, topic: &str) -> bool;
}

pub trait Attacher: Send + Sync {
    fn attach(&self, job: &str, config_dir: Option<&str>) -> Result<Box<dyn Pty>, ApiError>;
    fn screen_check(&self) -> &dyn ScreenCheck;
}

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

pub(crate) fn broad_folder(real: &std::path::Path, home: &std::path::Path) -> bool {
    let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    real.parent().is_none() || real == home
}

pub type Pause = Box<dyn Fn(u64) + Send + Sync>;

struct NoLaunch;

impl Launcher for NoLaunch {
    fn run(&self, _command: &Command, _timeout: Duration) -> Result<LaunchOutput, LaunchError> {
        Err(LaunchError::Unsupported)
    }
    fn run_pty(&self, _script: &PtyScript) -> Result<bool, LaunchError> {
        Err(LaunchError::Unsupported)
    }
}

pub struct ActingOps {
    pub(crate) acting: Arc<Acting>,
    pub(crate) prompts: PromptStore,
    pub(crate) sessions: Arc<Mutex<Value>>,
    pub(crate) roots: Roots,
    pub(crate) room: Arc<dyn Room>,
    attacher: Box<dyn Attacher>,
    pub(crate) launcher: Arc<dyn Launcher>,
    pub(crate) pause: Pause,
    typing: Mutex<()>,
    pub(crate) tick: Duration,
    pub(crate) parent_of: Parents,
    stopping: Mutex<HashMap<String, Instant>>,
    pub(crate) refused: Mutex<HashMap<PathBuf, Instant>>,
    agy: OnceLock<Arc<AgyOps>>,
    codex: OnceLock<Arc<CodexOps>>,
    catalog: OnceLock<Arc<CatalogService>>,
    screen_file: Option<PathBuf>,
    pub(crate) held: crate::held_ops::HeldState,
    pub(crate) events: Option<Events>,
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
            codex: OnceLock::new(),
            catalog: OnceLock::new(),
            screen_file: None,
            held: crate::held_ops::HeldState::default(),
            events: None,
        }
    }

    pub fn with_screen_file(mut self, file: PathBuf) -> ActingOps {
        self.screen_file = Some(file);
        self
    }

    pub fn acting_switch(&self) -> Arc<Acting> {
        self.acting.clone()
    }

    pub fn attach_agy(&self, agy: Arc<AgyOps>) {
        let _ = self.agy.set(agy);
    }

    pub fn attach_codex(&self, codex: Arc<CodexOps>) {
        let _ = self.codex.set(codex);
    }

    pub fn attach_catalog(&self, catalog: Arc<CatalogService>) {
        let _ = self.catalog.set(catalog);
    }

    pub(crate) fn catalog(&self) -> &CatalogService {
        self.catalog
            .get_or_init(|| Arc::new(CatalogService::built_in()))
    }

    #[cfg(test)]
    pub(crate) fn age_stopping(&self, by: Duration) {
        age(&self.stopping, by);
    }

    #[cfg(test)]
    pub(crate) fn age_refusals(&self, by: Duration) {
        age(&self.refused, by);
    }

    pub fn with_launcher(mut self, launcher: Arc<dyn Launcher>, pause: Pause) -> ActingOps {
        self.launcher = launcher;
        self.pause = pause;
        self
    }

    pub fn is_on(&self) -> bool {
        self.acting.is_on()
    }

    pub fn prompts(&self) -> &PromptStore {
        &self.prompts
    }

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
            if op == ops::SESSION_REPLY {
                self.note_acting_off(args, who(caller, args).via);
            }
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
                Ok(Value::Array(fit_list(list)))
            }
            ops::SESSION_QUESTION => self.question(caller, args),
            ops::HOOK_REQUEST => self.hook_request(caller, args),
            ops::SESSION_REPLY => self.reply(caller, args),
            ops::SESSION_PERMISSION => self.permission(caller, args),
            ops::SESSION_ANSWER => self.answer(caller, args),
            ops::SESSION_INTERRUPT | ops::SESSION_TAKE_BACK => self.controlled(caller, op, args),
            op if CONTROL_OPS.contains(&op) => self.controlled(caller, op, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn set_acting(&self, args: &Value) -> Result<Value, ApiError> {
        let on = args
            .get("on")
            .and_then(Value::as_bool)
            .ok_or_else(|| bad("settings.set_acting takes {\"on\": true|false}"))?;
        self.acting.set(on);
        self.note_acting(on, args);
        if !on {
            for prompt in self.prompts.drain() {
                self.resolved(&prompt, "pass", "acting_off");
            }
            if let Some(agy) = self.agy.get() {
                agy.acting_off();
            }
            if let Some(codex) = self.codex.get() {
                codex.acting_off();
            }
        }
        let data = json!({"acting": on});
        self.room
            .publish(topics::SETTINGS, topics::ACTING_CHANGED, &data);
        Ok(data)
    }

    pub(crate) fn agents_now(&self) -> Vec<Agent> {
        let list = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        list.as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| serde_json::from_value(r.clone()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn agent(&self, args: &Value) -> Result<Agent, ApiError> {
        let id = text_arg(args, "id")?;
        let list = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let row = list
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r.get("id") == Some(&json!(id))))
            .ok_or_else(|| bad(format!("no session {id}")))?;
        serde_json::from_value(row.clone()).map_err(|e| ApiError::internal(e.to_string()))
    }

    pub(crate) fn play(
        &self,
        agent: &Agent,
        plan: &Plan,
        log: &mut dyn QueueLog,
    ) -> Result<Outcome, PlayError> {
        let _one = self.typing.lock().unwrap_or_else(|e| e.into_inner());
        self.play_locked(agent, plan, log, None)
    }

    fn play_locked(
        &self,
        agent: &Agent,
        plan: &Plan,
        log: &mut dyn QueueLog,
        keep: Option<&Expected>,
    ) -> Result<Outcome, PlayError> {
        if plan.is_empty() {
            return Ok(Outcome::default());
        }
        let job = match &agent.target {
            AgentTarget::Job(job) if agent.can_reply() => job.clone(),
            _ => return Err(bad("this session takes no replies").into()),
        };
        let config_dir = config_dir_for_job(&self.roots, &job).or_else(|| agent.config_dir.clone());
        let mut pty = self
            .attacher
            .attach(&job, config_dir.as_deref())
            .map_err(|err| PlayError {
                why: attach_why(&err),
                err,
            })?;
        let played = execute(plan, pty.as_mut(), self.attacher.screen_check(), log);
        if let (Err(failure), Some(expect), Some(file)) = (&played, keep, &self.screen_file) {
            if failure.is_permission_refusal() {
                keep_screen(file, expect, failure, &pty.screen());
            }
        }
        played.map_err(|f| PlayError {
            why: why_of(&f),
            err: ApiError::new(ErrorCode::Failed, f.to_string()),
        })
    }

    fn reply(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let text = text_arg(args, "text")?;
        if text.trim().is_empty() {
            return Err(bad("the reply is empty"));
        }
        let agent = self.agent(args)?;
        let via = who(caller, args).via;
        let session = session_ref(&agent);
        let chars = text.chars().count();
        let by_agent = caller.tier == Tier::Agent;
        if by_agent && !self.note_reply(session.clone(), via, chars, None, None, true) {
            return Err(log_down());
        }
        let started = Instant::now();
        let sent = self.reply_or_hold(&agent, text, crate::held_ops::now_ms());
        match &sent {
            Ok(value) if value.get("state").and_then(Value::as_str) == Some("queued") => {
                self.note_held(session, HeldThen::SentAtTurnEnd);
            }
            Ok(_) if !by_agent => {
                let waited = u64::try_from(started.elapsed().as_millis()).ok();
                self.note_reply(session, via, chars, None, waited, false);
            }
            Ok(_) => {}
            Err(failure) => {
                self.note_reply(session, via, chars, Some(failure.why), None, false);
            }
        }
        sent.map_err(ApiError::from)
    }

    fn controlled(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        let target = self.target_of(op, args);
        let by_agent = caller.tier == Tier::Agent;
        if let (true, Some(target)) = (by_agent, &target) {
            if !self.note_control(target, DoneOrFailed::Done, true) {
                return Err(log_down());
            }
        }
        let result = match op {
            ops::SESSION_INTERRUPT => self.interrupt(args),
            ops::SESSION_TAKE_BACK => self.take_back(args),
            _ => self.control(op, args),
        };
        if let Some(target) = &target {
            match (&result, by_agent) {
                (Ok(_), false) => {
                    self.note_control(target, DoneOrFailed::Done, false);
                }
                (Err(_), _) => {
                    self.note_control(target, DoneOrFailed::Failed, false);
                }
                _ => {}
            }
        }
        result
    }

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
        self.play_locked(&agent, &plan, &mut NoLog, None)?;
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

    fn held(&self, args: &Value) -> Result<Option<Prompt>, ApiError> {
        Ok(self.prompts.get(text_arg(args, "id")?))
    }

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

    fn permission(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let decision = text_arg(args, "decision")?;
        let who = who(caller, args);
        let Some(prompt) = self.held(args)? else {
            if args.get("expect").is_some() {
                return self.permission_on_screen(args, decision, &who);
            }
            return Err(bad(
                "no prompt with that id is waiting (answer by the prompt's id, or pass expect to answer a row's dialog)",
            ));
        };
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
        self.note_prompt(&prompt, answer_of(decision), &who);
        Ok(json!({"decided": decision, "promptId": prompt.id}))
    }

    fn note_prompt(&self, prompt: &Prompt, answer: AnswerKind, who: &Who) {
        let kind = match prompt.kind {
            Kind::Question => PromptKind::Question,
            Kind::Permission => PromptKind::Permission,
        };
        let tool = prompt.event.get("tool_name").and_then(Value::as_str);
        let waited = u64::try_from(now_ms().saturating_sub(prompt.created_ms)).ok();
        self.note_answered(
            Ref::<Session>::new(&prompt.session_id),
            kind,
            tool,
            answer,
            who,
            waited,
        );
    }

    fn permission_on_screen(
        &self,
        args: &Value,
        decision: &str,
        who: &Who,
    ) -> Result<Value, ApiError> {
        let spoken = decision;
        let decision =
            Decision::parse(decision).ok_or_else(|| bad("decision is allow, deny or always"))?;
        if args.get("message").is_some() {
            return Err(bad(
                "a deny message can't be typed into the terminal's dialog; deny without one",
            ));
        }
        let expect = expect_arg(args)?;
        let agent = self.agent(args)?;
        {
            let _one = self.typing.lock().unwrap_or_else(|e| e.into_inner());
            self.play_locked(
                &agent,
                &plan::permission(decision, &expect),
                &mut NoLog,
                Some(&expect),
            )?;
        }
        self.note_answered(
            session_ref(&agent),
            PromptKind::Permission,
            Some(&expect.tool),
            answer_of(spoken),
            who,
            None,
        );
        Ok(json!({"decided": decision.word(), "answered": "terminal"}))
    }

    fn answer(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let who = who(caller, args);
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
            self.note_prompt(&prompt, typed_or_picked(answers), &who);
            return Ok(json!({"answered": "hook", "promptId": prompt.id}));
        }
        let agent = self.agent(args)?;
        let question = question_arg(args)?;
        let choices = choices_from_json(answers, &question.items).map_err(bad)?;
        self.play(&agent, &plan::answer(&choices, &question), &mut NoLog)?;
        self.note_answered(
            session_ref(&agent),
            PromptKind::Question,
            None,
            typed_or_picked(answers),
            &who,
            None,
        );
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

    fn question(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if let Some(prompt) = self.held_for_reading(args)? {
            if let Some(q) = &prompt.question {
                let mut out = question_json(q);
                out["promptId"] = json!(prompt.id);
                return Ok(out);
            }
        }
        if !self.acting.is_on() {
            return Err(forbidden(ACTING_OFF));
        }
        if caller.tier != Tier::App {
            return Err(forbidden(
                "reading a question off the screen moves its tabs: only the app does it",
            ));
        }
        let agent = self.agent(args)?;
        let job = match &agent.target {
            AgentTarget::Job(job) if agent.can_reply() => job.clone(),
            _ => return Err(bad("this session has no terminal to read")),
        };
        let _one = self.typing.lock().unwrap_or_else(|e| e.into_inner());
        let config_dir = config_dir_for_job(&self.roots, &job).or_else(|| agent.config_dir.clone());
        let mut pty = self.attacher.attach(&job, config_dir.as_deref())?;
        let read = read_question(pty.as_mut(), &job)
            .map_err(|f| ApiError::new(ErrorCode::Failed, f.to_string()))?;
        Ok(match read {
            Some(q) => {
                let mut out = question_json(&q);
                out["id"] = json!(q.tool_use_id);
                out
            }
            None => json!({"items": []}),
        })
    }
}

fn answer_of(decision: &str) -> AnswerKind {
    match decision {
        "always" => AnswerKind::AllowAlways,
        "deny" => AnswerKind::Deny,
        _ => AnswerKind::AllowOnce,
    }
}

fn typed_or_picked(answers: &Value) -> AnswerKind {
    if answers
        .as_array()
        .is_some_and(|list| list.iter().any(Value::is_string))
    {
        AnswerKind::Typed
    } else {
        AnswerKind::Picked
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

fn expect_arg(args: &Value) -> Result<Expected, ApiError> {
    let expect = args
        .get("expect")
        .filter(|e| e.is_object())
        .ok_or_else(|| bad("expect is {\"tool\", \"summary\"}"))?;
    let tool = expect
        .get("tool")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| bad("expect.tool is needed"))?;
    let summary = match expect.get("summary") {
        None | Some(Value::Null) => "",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return Err(bad("expect.summary is a text")),
    };
    Ok(Expected {
        tool: tool.to_string(),
        summary: summary.to_string(),
    })
}

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

fn keep_screen(file: &Path, expect: &Expected, failure: &Failure, lines: &[String]) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::remove_file(file);
    let Ok(mut out) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(file)
    else {
        return;
    };
    let _ = writeln!(out, "request: {}: {}", expect.tool, expect.summary);
    let _ = writeln!(out, "refused: {failure}");
    let _ = writeln!(out, "----- screen ({} lines) -----", lines.len());
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
}
