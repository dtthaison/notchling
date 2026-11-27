use crate::acting_ops::now_ms;
use crate::consent_ops::{options, retry_of, ConsentOps, Handler, Opening};
use crate::op_group::OpGroup;
use crate::session_children::{Child, Children};
use notchling_engine::consent::{Consent, SESSION, SESSION_STOP};
use notchling_events::kinds::choices::Answerer;
use notchling_local_api::{ops, AgentIdentity, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::sync::{Arc, Weak};

pub const MESSAGE_BYTES: usize = 20_000;
const START_USAGE: &str = "session.start takes {\"folder\": \"/absolute/path\", \"tool\": \"claude\"|\"codex\"|\"agy\", \"model\": text, \"effort\": text, \"message\": text}";
const SAY_USAGE: &str = "session.say takes {\"id\": text, \"text\": text}";
const HALT_USAGE: &str = "session.halt takes {\"id\": text, \"how\": \"stop\"|\"interrupt\"}";
const DISCARD_USAGE: &str = "session.discard takes {\"id\": text}";
const ACTING_OFF: &str = "Act on Sessions is off, so no agent may start or drive a session.";
const OTHER_ORG: &str = "That session is in another organization.";
const NEVER_PROMPTS: &str = "Only you can skip permission prompts, in Notchling.";
const SKIP_KEYS: [&str; 6] = [
    "permissionMode",
    "permission_mode",
    "skipPermissions",
    "dangerouslySkipPermissions",
    "flags",
    "yolo",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Start,
    Say,
    Stop,
    Interrupt,
    Remove,
    Choose,
    Run,
    Pick,
    Browse,
    Tune,
}

const ALL_ACTIONS: [Action; 10] = [
    Action::Start,
    Action::Say,
    Action::Stop,
    Action::Interrupt,
    Action::Remove,
    Action::Choose,
    Action::Run,
    Action::Pick,
    Action::Browse,
    Action::Tune,
];

impl Action {
    pub fn word(self) -> &'static str {
        match self {
            Action::Start => "start",
            Action::Say => "say",
            Action::Stop => "stop",
            Action::Interrupt => "interrupt",
            Action::Remove => "remove",
            Action::Choose => "choose",
            Action::Run => "run",
            Action::Pick => "pick",
            Action::Browse => "browse",
            Action::Tune => "tune",
        }
    }

    pub fn parse(word: &str) -> Option<Action> {
        ALL_ACTIONS.into_iter().find(|a| a.word() == word)
    }

    fn drives(self) -> bool {
        matches!(
            self,
            Action::Choose | Action::Run | Action::Pick | Action::Browse | Action::Tune
        )
    }

    fn kind(self) -> &'static str {
        match self {
            Action::Stop | Action::Interrupt => SESSION_STOP,
            _ => SESSION,
        }
    }

    fn op(self) -> &'static str {
        match self {
            Action::Start => ops::SESSION_START,
            Action::Say => ops::SESSION_SAY,
            Action::Stop | Action::Interrupt => ops::SESSION_HALT,
            Action::Remove => ops::SESSION_DISCARD,
            Action::Choose => ops::SESSION_CHOOSE,
            Action::Run => ops::SESSION_RUN,
            Action::Pick => ops::SESSION_PICK,
            Action::Browse => ops::SESSION_BROWSE,
            Action::Tune => ops::SESSION_TUNE,
        }
    }

    fn line(self, change: &Value) -> &'static str {
        match self {
            Action::Start => "consent-session-start",
            Action::Say => "consent-session-say",
            Action::Stop => "consent-session-stop",
            Action::Interrupt => "consent-session-interrupt",
            Action::Remove => "consent-session-remove",
            Action::Choose => "consent-session-choose",
            Action::Run => "consent-session-run",
            Action::Pick if change["dismiss"] == json!(true) => "consent-session-dismiss",
            Action::Pick => "consent-session-pick",
            Action::Browse if change["on"] == json!(true) => "consent-session-browse-on",
            Action::Browse => "consent-session-browse-off",
            Action::Tune => "consent-session-tune",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub id: String,
    pub tool: &'static str,
    pub folder: String,
    pub name: String,
    pub remote: bool,
    pub also: Vec<String>,
}

pub trait Directory: Send + Sync {
    fn find(&self, id: &str) -> Option<Target>;
    fn organization(&self, folder: &str) -> String;
    fn trusted(&self, tool: &str, folder: &str) -> bool;
    fn folder_exists(&self, folder: &str) -> bool;
}

pub type Runner = Arc<dyn Fn(&str, &Value) -> Result<Value, ApiError> + Send + Sync>;
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
pub type ActingOn = Arc<dyn Fn() -> bool + Send + Sync>;

pub fn tool_word(word: &str) -> Option<&'static str> {
    match word {
        "claude" => Some("claude"),
        "codex" => Some("codex"),
        "agy" | "antigravity" => Some("antigravity"),
        _ => None,
    }
}

pub fn tool_of_id(id: &str) -> &'static str {
    if id.starts_with("codex:") {
        "codex"
    } else if id.starts_with("agy:") {
        "antigravity"
    } else {
        "claude"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Relation {
    Own,
    Child,
    Other,
}

pub struct SessionAsks {
    me: Weak<SessionAsks>,
    consent: Arc<ConsentOps>,
    dir: Arc<dyn Directory>,
    run: Runner,
    children: Arc<Children>,
    acting: ActingOn,
    clock: Clock,
}

fn dry_run(args: &Value) -> bool {
    args.get("dryRun").and_then(Value::as_bool) == Some(true)
}

fn bad(text: &str) -> ApiError {
    ApiError::bad_request(text)
}

fn forbidden(text: &str, reason: &str) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, text).with_reason(reason)
}

fn acting_off() -> ApiError {
    forbidden(ACTING_OFF, "switch_off")
        .with_field("switch", json!("acting"))
        .with_field(
            "ask",
            json!({"command": "notchling switch on acting", "kind": "switch"}),
        )
}

fn text_of<'a>(args: &'a Value, key: &str, usage: &str) -> Result<&'a str, ApiError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| bad(usage))
}

fn word_ok(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 80
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/' | '['))
}

fn message_of(args: &Value, key: &str, usage: &str) -> Result<String, ApiError> {
    let text = text_of(args, key, usage)?;
    if text.len() > MESSAGE_BYTES {
        return Err(bad(&format!(
            "The message is over {} KB.",
            MESSAGE_BYTES / 1000
        )));
    }
    Ok(text.to_string())
}

impl SessionAsks {
    pub fn new(
        consent: Arc<ConsentOps>,
        dir: Arc<dyn Directory>,
        run: Runner,
        children: Arc<Children>,
        acting: ActingOn,
    ) -> Arc<SessionAsks> {
        Self::with_clock(consent, dir, run, children, acting, Arc::new(now_ms))
    }

    pub fn with_clock(
        consent: Arc<ConsentOps>,
        dir: Arc<dyn Directory>,
        run: Runner,
        children: Arc<Children>,
        acting: ActingOn,
        clock: Clock,
    ) -> Arc<SessionAsks> {
        let asks = Arc::new_cyclic(|me| SessionAsks {
            me: me.clone(),
            consent: consent.clone(),
            dir,
            run,
            children,
            acting,
            clock,
        });
        consent.register(SESSION, Arc::new(Apply(asks.me.clone())));
        consent.register(SESSION_STOP, Arc::new(Apply(asks.me.clone())));
        asks
    }

    pub fn children(&self) -> &Arc<Children> {
        &self.children
    }

    fn relation(&self, who: &AgentIdentity, target: &Target) -> Relation {
        let Some(session) = who.session.as_deref().filter(|_| who.verified) else {
            return Relation::Other;
        };
        if target.id == session || target.also.iter().any(|a| a == session) {
            return Relation::Own;
        }
        if self.children.owned_by(session, &target.id).is_some() {
            return Relation::Child;
        }
        Relation::Other
    }

    fn check_org(&self, who: &AgentIdentity, folder: &str) -> Result<String, ApiError> {
        let theirs = self.dir.organization(folder);
        if who.verified {
            if let Some(mine) = &who.organization {
                if *mine != theirs {
                    return Err(forbidden(OTHER_ORG, "other_organization"));
                }
            }
        }
        Ok(theirs)
    }

    fn target_of(&self, id: &str) -> Result<Target, ApiError> {
        let target = self
            .dir
            .find(id)
            .ok_or_else(|| bad("No such session.").with_reason("no_session"))?;
        if target.remote {
            return Err(bad("That session runs on another machine.").with_reason("not_yet"));
        }
        Ok(target)
    }

    fn facts(&self, action: Action, change: &Value, trusts: bool) -> Value {
        let mut lines = vec![json!({"line": action.line(change), "args": {
            "tool": change["tool"], "folder": change["folder"], "name": change["name"],
        }})];
        if let Some(model) = change["model"].as_str().filter(|_| action == Action::Start) {
            lines.push(json!({"line": "consent-session-model", "args": {"model": model}}));
        }
        if trusts {
            lines.push(json!({"line": "consent-session-trusts", "args": {}}));
        }
        if action == Action::Start {
            lines.push(json!({"line": "consent-session-child", "args": {}}));
        }
        Value::Array(lines)
    }

    fn open(
        &self,
        caller: &Caller,
        args: &Value,
        action: Action,
        target: &str,
        change: Value,
        trusts: bool,
    ) -> Result<Value, ApiError> {
        let (reason, expires_in_ms, key) = options(args);
        let op = action.op();
        let opening = Opening {
            parts: None,
            kind: action.kind(),
            target: target.to_string(),
            facts: self.facts(action, &change, trusts),
            request: json!({"op": op, "action": action.word(), "target": target}),
            change,
            reason,
            expires_in_ms,
            key,
            retry_of: retry_of(args),
        };
        if dry_run(args) {
            let would = self.consent.would(&opening)?;
            let mut change = opening.change.clone();
            if action == Action::Choose {
                if let Some(map) = change.as_object_mut() {
                    map.remove("message");
                    map.remove("question");
                }
            }
            return Ok(json!({"dryRun": true, "op": op, "would": "ask",
                "change": change, "request": would, "error": null}));
        }
        let opened = self.consent.open(caller, opening)?;
        let row = opened.row();
        Ok(json!({"state": row.state.as_str(), "request": self.consent.project(row, true)}))
    }

    fn gate(&self, args: &Value) -> Result<(), ApiError> {
        if SKIP_KEYS.iter().any(|k| args.get(k).is_some()) {
            return Err(forbidden(NEVER_PROMPTS, "never"));
        }
        if !(self.acting)() {
            return Err(acting_off());
        }
        Ok(())
    }

    fn start(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        self.gate(args)?;
        let folder = text_of(args, "folder", START_USAGE)?;
        if !folder.starts_with('/') || folder.contains('\0') {
            return Err(bad(START_USAGE));
        }
        let tool = match args.get("tool").and_then(Value::as_str) {
            None => "claude",
            Some(word) => tool_word(word).ok_or_else(|| bad(START_USAGE))?,
        };
        if tool != "claude" {
            return Err(
                bad("Starting a Codex or Antigravity session isn't built yet.")
                    .with_reason("not_yet"),
            );
        }
        let pick = |key: &str| -> Result<Option<String>, ApiError> {
            match args.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) if word_ok(s) => Ok(Some(s.clone())),
                _ => Err(bad(START_USAGE)),
            }
        };
        let (model, effort) = (pick("model")?, pick("effort")?);
        if effort.is_some() {
            return Err(
                bad("Claude Code sessions take a model, not an effort.").with_reason("usage")
            );
        }
        let message = match args.get("message") {
            None | Some(Value::Null) => None,
            Some(_) => Some(message_of(args, "message", START_USAGE)?),
        };
        let who = self.consent.who(caller);
        let organization = self.check_org(&who, folder)?;
        if !self.dir.folder_exists(folder) {
            return Err(bad("That folder doesn't exist.").with_reason("bad_value"));
        }
        let trusts = !self.dir.trusted(tool, folder);
        let change = json!({
            "action": "start", "tool": tool, "folder": folder, "name": folder,
            "model": model, "message": message, "organization": organization,
            "trusts": trusts,
        });
        self.open(caller, args, Action::Start, folder, change, trusts)
    }

    fn change_of(
        target: &Target,
        action: Action,
        text: Option<&str>,
        organization: String,
    ) -> Value {
        json!({
            "action": action.word(), "tool": target.tool, "folder": target.folder,
            "name": target.name, "session": target.id, "organization": organization,
            "message": text,
        })
    }

    fn free(
        &self,
        args: &Value,
        target: &Target,
        action: Action,
        text: Option<&str>,
        organization: String,
    ) -> Result<Value, ApiError> {
        if dry_run(args) {
            self.map(target.tool, action, &target.id, text)?;
            return Ok(json!({"dryRun": true, "op": action.op(), "would": "apply",
                "change": Self::change_of(target, action, text, organization),
                "request": null, "error": null}));
        }
        let result = self.execute(target, action, text)?;
        Ok(json!({"free": true, "action": action.word(), "session": target.id, "result": result}))
    }

    fn card(
        &self,
        caller: &Caller,
        args: &Value,
        target: &Target,
        action: Action,
        text: Option<&str>,
        organization: String,
    ) -> Result<Value, ApiError> {
        self.map(target.tool, action, &target.id, text)?;
        let change = Self::change_of(target, action, text, organization);
        self.open(caller, args, action, &target.id, change, false)
    }

    fn drive(&self, caller: &Caller, args: &Value, action: Action) -> Result<Value, ApiError> {
        self.gate(args)?;
        let usage = match action {
            Action::Say => SAY_USAGE,
            Action::Remove => DISCARD_USAGE,
            _ => HALT_USAGE,
        };
        let id = text_of(args, "id", usage)?;
        let text = if action == Action::Say {
            Some(message_of(args, "text", usage)?)
        } else {
            None
        };
        let target = self.target_of(id)?;
        let who = self.consent.who(caller);
        let organization = self.check_org(&who, &target.folder)?;
        let relation = self.relation(&who, &target);
        let parent = who.session.clone().filter(|_| who.verified);
        match (action, relation) {
            (Action::Say | Action::Remove, Relation::Own) => {
                Err(bad("A session can't do that to itself.").with_reason("usage"))
            }
            (Action::Say, Relation::Child) => {
                let parent = parent.unwrap_or_default();
                let now = (self.clock)();
                let room = if dry_run(args) {
                    self.children.has_free(&parent, now)
                } else {
                    self.children.take_free(&parent, now)
                };
                if !room {
                    let wait = self.children.retry_after_ms(&parent, now);
                    return Err(ApiError::new(
                        ErrorCode::Failed,
                        "That is 30 messages this hour to your sessions. Try again later.",
                    )
                    .with_reason("busy")
                    .with_field("retryAfterMs", json!(wait)));
                }
                self.free(args, &target, action, text.as_deref(), organization)
            }
            (Action::Stop | Action::Interrupt, Relation::Own | Relation::Child) => {
                self.free(args, &target, action, None, organization)
            }
            (Action::Remove, Relation::Child) => {
                let stopped = self
                    .children
                    .get(&target.id)
                    .is_some_and(|c| c.stopped_ms.is_some());
                if !stopped {
                    return Err(bad("Stop that session first.").with_reason("usage"));
                }
                self.free(args, &target, action, None, organization)
            }
            _ => self.card(caller, args, &target, action, text.as_deref(), organization),
        }
    }

    fn halt(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        match args.get("how").and_then(Value::as_str) {
            Some("stop") => self.drive(caller, args, Action::Stop),
            Some("interrupt") => self.drive(caller, args, Action::Interrupt),
            _ => Err(bad(HALT_USAGE)),
        }
    }

    fn map(
        &self,
        tool: &str,
        action: Action,
        id: &str,
        text: Option<&str>,
    ) -> Result<(String, Value), ApiError> {
        let sent = |text: Option<&str>| json!({"id": id, "text": text.unwrap_or_default()});
        Ok(match (tool, action) {
            ("claude", Action::Say) => (ops::SESSION_SEND.into(), sent(text)),
            ("codex", Action::Say) => (ops::CODEX_SEND.into(), sent(text)),
            ("antigravity", Action::Say) => (ops::AGY_SEND.into(), sent(text)),
            ("claude", Action::Stop) => (ops::SESSION_STOP.into(), json!({"id": id})),
            ("codex", Action::Stop | Action::Interrupt) => {
                (ops::CODEX_STOP.into(), json!({"id": id}))
            }
            ("antigravity", Action::Stop | Action::Interrupt) => {
                (ops::AGY_CANCEL.into(), json!({"id": id}))
            }
            ("claude", Action::Interrupt) => (ops::SESSION_INTERRUPT.into(), json!({"id": id})),
            ("claude", Action::Remove) => (ops::SESSION_REMOVE.into(), json!({"id": id})),
            ("codex", Action::Remove) => (ops::CODEX_DELETE.into(), json!({"id": id})),
            _ => {
                return Err(
                    bad("That can't be done to this kind of session.").with_reason("not_yet")
                )
            }
        })
    }

    fn execute(
        &self,
        target: &Target,
        action: Action,
        text: Option<&str>,
    ) -> Result<Value, ApiError> {
        let (op, args) = self.map(target.tool, action, &target.id, text)?;
        let result = (self.run)(&op, &args)?;
        if action == Action::Stop {
            self.children.stopped(&target.id, (self.clock)());
        }
        if action == Action::Remove {
            self.children.forget(&target.id);
        }
        Ok(result)
    }

    fn run_start(&self, row: &Consent) -> Result<Value, String> {
        let change = &row.change;
        let folder = change["folder"].as_str().unwrap_or_default();
        let mut args = json!({"folder": folder});
        if let Some(model) = change["model"].as_str() {
            args["flags"] = json!(["--model", model]);
        }
        let mut started = (self.run)(ops::SESSION_NEW, &args);
        if started.is_err() && change["trusts"] == json!(true) {
            (self.run)(ops::FOLDER_TRUST, &json!({"folder": folder})).map_err(|e| e.message)?;
            started = (self.run)(ops::SESSION_NEW, &args);
        }
        let started = started.map_err(|e| e.message)?;
        let id = started["jobId"]
            .as_str()
            .ok_or_else(|| "the session didn't start".to_string())?
            .to_string();
        if let Some(parent) = row.session.clone().filter(|_| row.verified) {
            self.children.record(
                &id,
                Child {
                    started_by: parent,
                    tool: "claude".to_string(),
                    folder: folder.to_string(),
                    started_ms: (self.clock)(),
                    stopped_ms: None,
                },
            );
        }
        let queued = match change["message"].as_str() {
            Some(text) => (self.run)(ops::SESSION_SEND, &json!({"id": id, "text": text})).is_ok(),
            None => true,
        };
        Ok(json!({"session": id, "messageQueued": queued}))
    }

    fn apply_row(&self, row: &Consent) -> Result<Value, String> {
        let action = row.change["action"]
            .as_str()
            .and_then(Action::parse)
            .ok_or_else(|| "not a session action".to_string())?;
        if !(self.acting)() {
            return Err("Act on Sessions is off".to_string());
        }
        if action == Action::Start {
            return self.run_start(row);
        }
        let id = row.change["session"].as_str().unwrap_or_default();
        let target = self
            .dir
            .find(id)
            .ok_or_else(|| "the session is gone".to_string())?;
        if action.drives() {
            return self.act(action, row, id).map_err(|e| e.message);
        }
        self.execute(&target, action, row.change["message"].as_str())
            .map_err(|e| e.message)
    }

    fn still_fits(&self, row: &Consent) -> Result<(), String> {
        if !(self.acting)() {
            return Err("Act on Sessions is off".to_string());
        }
        match row.change["action"].as_str() {
            Some("start") => {
                let folder = row.change["folder"].as_str().unwrap_or_default();
                if self.dir.folder_exists(folder) {
                    Ok(())
                } else {
                    Err("the folder is gone".to_string())
                }
            }
            _ => {
                let id = row.change["session"].as_str().unwrap_or_default();
                self.dir
                    .find(id)
                    .ok_or_else(|| "the session is gone".to_string())?;
                self.unchanged(row, id)
            }
        }
    }
}

struct Apply(Weak<SessionAsks>);

impl Handler for Apply {
    fn recheck(&self, row: &Consent) -> Result<(), String> {
        match self.0.upgrade() {
            Some(asks) => asks.still_fits(row),
            None => Err("stopping".to_string()),
        }
    }

    fn apply(
        &self,
        row: &Consent,
        _by: Option<Answerer>,
        _device: Option<&str>,
    ) -> Result<Value, String> {
        match self.0.upgrade() {
            Some(asks) => asks.apply_row(row),
            None => Err("stopping".to_string()),
        }
    }

    fn shape(&self, row: &Consent, view: &mut Value, agent: bool) {
        let answer = row.change["action"] == json!("choose");
        if agent && (answer || !row.state.open()) {
            if let Some(change) = view.get_mut("change").and_then(Value::as_object_mut) {
                change.remove("message");
                change.remove("question");
            }
        }
    }
}

impl OpGroup for SessionAsks {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        matches!(
            op,
            ops::SESSION_START
                | ops::SESSION_SAY
                | ops::SESSION_HALT
                | ops::SESSION_DISCARD
                | ops::SESSION_CHOOSE
                | ops::SESSION_RUN
                | ops::SESSION_PICK
                | ops::SESSION_BROWSE
                | ops::SESSION_TUNE
        )
        .then_some(OpSpec::AGENT_ACTS)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::SESSION_START => self.start(caller, args),
            ops::SESSION_SAY => self.drive(caller, args, Action::Say),
            ops::SESSION_HALT => self.halt(caller, args),
            ops::SESSION_DISCARD => self.drive(caller, args, Action::Remove),
            ops::SESSION_CHOOSE => self.choose(caller, args),
            ops::SESSION_RUN => self.run_command(caller, args),
            ops::SESSION_PICK => self.pick(caller, args),
            ops::SESSION_BROWSE => self.browse(caller, args),
            ops::SESSION_TUNE => self.tune(caller, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

#[path = "session_acts.rs"]
mod acts;

#[cfg(test)]
#[path = "session_asks_tests.rs"]
mod tests;
