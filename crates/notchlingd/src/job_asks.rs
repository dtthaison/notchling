use crate::acting_ops::now_ms;
use crate::cluster_dispatch::build;
use crate::consent_ops::{options, retry_of, ConsentOps, Failure, Handler, Opening};
use crate::job_book::{JobBook, Record};
use crate::op_group::OpGroup;
use notchling_engine::bridge::net::crypto::sha256_hex;
use notchling_engine::cluster::jobs::job_id;
use notchling_engine::cluster::wire::OriginTier;
use notchling_engine::consent::{Consent, DISPATCH, JOB, JOB_STOP};
use notchling_engine::delegate::record::pid_alive;
use notchling_engine::organizations::PERSONAL_ID;
use notchling_events::kinds::choices::Answerer;
use notchling_local_api::{ops, AgentIdentity, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::sync::{Arc, Weak};

const TAIL_USAGE: &str = "jobs.tail takes {\"id\": text, \"from\": number}";
const RETURN_USAGE: &str = "jobs.return takes {\"id\": text}";
const STOP_USAGE: &str = "jobs.stop takes {\"id\": text}";
const BEGIN_USAGE: &str = "delegate.begin takes {\"tool\": text, \"folder\": \"/absolute/path\"}";
const END_USAGE: &str = "delegate.end takes {\"id\": text, \"exit\": number}";
const DISPATCH_USAGE: &str = "cluster.dispatch takes {\"machine\": name, \"key\": text, \"tool\": claude|codex|agy|shell, \"cwd\": path, \"model\": text, \"effort\": text, \"text\": text, \"command\": text, \"secrets\": [names]}";
const CLUSTER_OFF: &str = "Cluster is off, so no agent may send a job to another machine.";
const OTHER_ORG: &str = "That machine or secret is in another organization.";
const NO_SESSION: &str = "Only an agent inside a Notchling session may do that.";
const KEY_IN_USE: &str = "That key already started another job. Use a new key.";
const DELEGATE_USAGE: &str =
    "cluster.dispatch takes {\"node\": id, \"key\": text, \"delegate\": [args]}";
const NO_JOB: &str = "No such job.";
pub const TAIL_TEXT: usize = 16 * 1024;

pub(crate) const TERMINAL: [&str; 4] = ["done", "failed", "interrupted", "lost"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub organization: String,
    pub online: bool,
    pub me: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobRow {
    pub id: String,
    pub node: String,
    pub machine: String,
    pub organization: String,
    pub tool: String,
    pub folder: String,
    pub state: String,
    pub created_ms: i64,
    pub session: Option<String>,
    pub detail: Option<String>,
    pub secrets: Vec<String>,
    pub local: bool,
}

pub trait Fleet: Send + Sync {
    fn member(&self, name_or_id: &str) -> Option<Member>;
    fn jobs(&self) -> Vec<JobRow>;
    fn secret_organization(&self, name: &str) -> Option<String>;
}

pub trait Hands: Send + Sync {
    fn dispatch(&self, args: &Value) -> Result<Value, ApiError>;
    fn tail(&self, job: &JobRow, from: u64) -> Result<Value, ApiError>;
    fn give_back(&self, job: &JobRow) -> Result<Value, ApiError>;
    fn halt(&self, job: &JobRow) -> Result<Value, ApiError>;
    fn can_halt(&self, _job: &JobRow) -> bool {
        true
    }
}

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
pub type SwitchOn = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Tail,
    Return,
    Stop,
}

impl Act {
    fn word(self) -> &'static str {
        match self {
            Act::Tail => "tail",
            Act::Return => "return",
            Act::Stop => "stop",
        }
    }

    fn parse(word: &str) -> Option<Act> {
        [Act::Tail, Act::Return, Act::Stop]
            .into_iter()
            .find(|a| a.word() == word)
    }

    fn kind(self) -> &'static str {
        match self {
            Act::Stop => JOB_STOP,
            _ => JOB,
        }
    }

    fn op(self) -> &'static str {
        match self {
            Act::Tail => ops::JOBS_TAIL,
            Act::Return => ops::JOBS_RETURN,
            Act::Stop => ops::JOBS_STOP,
        }
    }

    fn line(self) -> &'static str {
        match self {
            Act::Tail => "consent-job-tail",
            Act::Return => "consent-job-return",
            Act::Stop => "consent-job-stop",
        }
    }
}

struct Draft {
    kind: &'static str,
    op: &'static str,
    target: String,
    change: Value,
    facts: Value,
    key: Option<String>,
}

pub struct JobAsks {
    me: Weak<JobAsks>,
    consent: Arc<ConsentOps>,
    fleet: Arc<dyn Fleet>,
    hands: Arc<dyn Hands>,
    book: Arc<JobBook>,
    cluster_on: SwitchOn,
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

fn cluster_off() -> ApiError {
    forbidden(CLUSTER_OFF, "switch_off")
        .with_field("switch", json!("cluster"))
        .with_field(
            "ask",
            json!({"command": "notchling switch on cluster", "kind": "switch"}),
        )
}

fn text_of<'a>(args: &'a Value, key: &str, usage: &str) -> Result<&'a str, ApiError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| bad(usage))
}

pub fn is_terminal(state: &str) -> bool {
    TERMINAL.contains(&state)
}

fn caller_of(who: &AgentIdentity) -> Result<(String, String), ApiError> {
    let session = who
        .session
        .clone()
        .filter(|_| who.verified)
        .ok_or_else(|| forbidden(NO_SESSION, "no_session"))?;
    let organization = who
        .organization
        .clone()
        .unwrap_or_else(|| PERSONAL_ID.to_string());
    Ok((session, organization))
}

impl JobAsks {
    pub fn new(
        consent: Arc<ConsentOps>,
        fleet: Arc<dyn Fleet>,
        hands: Arc<dyn Hands>,
        book: Arc<JobBook>,
        cluster_on: SwitchOn,
    ) -> Arc<JobAsks> {
        Self::with_clock(consent, fleet, hands, book, cluster_on, Arc::new(now_ms))
    }

    pub fn with_clock(
        consent: Arc<ConsentOps>,
        fleet: Arc<dyn Fleet>,
        hands: Arc<dyn Hands>,
        book: Arc<JobBook>,
        cluster_on: SwitchOn,
        clock: Clock,
    ) -> Arc<JobAsks> {
        let asks = Arc::new_cyclic(|me| JobAsks {
            me: me.clone(),
            consent: consent.clone(),
            fleet,
            hands,
            book,
            cluster_on,
            clock,
        });
        for kind in [DISPATCH, JOB, JOB_STOP] {
            consent.register(kind, Arc::new(Apply(asks.me.clone())));
        }
        asks.reconcile();
        asks
    }

    pub fn book(&self) -> &Arc<JobBook> {
        &self.book
    }

    fn gate(&self) -> Result<(), ApiError> {
        if (self.cluster_on)() {
            Ok(())
        } else {
            Err(cluster_off())
        }
    }

    fn same_org(&self, who: &AgentIdentity, theirs: &str) -> Result<(), ApiError> {
        let (_, mine) = caller_of(who)?;
        if mine == theirs {
            Ok(())
        } else {
            Err(forbidden(OTHER_ORG, "other_organization"))
        }
    }

    fn key_taken(&self, session: &str, id: &str) -> bool {
        match self.book.get(id) {
            Some(record) => record.session != session,
            None => self.fleet.jobs().iter().any(|j| j.id == id),
        }
    }

    pub fn delegate(&self, caller: &Caller, args: &Value) -> Result<Option<Value>, ApiError> {
        self.gate()?;
        let who = self.consent.who(caller);
        let asked = args
            .get("node")
            .or_else(|| args.get("machine"))
            .and_then(Value::as_str)
            .filter(|m| !m.trim().is_empty())
            .ok_or_else(|| bad(DELEGATE_USAGE))?;
        let member = self
            .fleet
            .member(asked)
            .ok_or_else(|| bad("No such machine in the cluster.").with_reason("no_machine"))?;
        self.same_org(&who, &member.organization)?;
        if dry_run(args) {
            return Ok(Some(json!({"dryRun": true, "op": ops::CLUSTER_DISPATCH,
                "would": "apply", "change": {"machine": member.name, "delegate": true},
                "request": null, "error": null})));
        }
        Ok(None)
    }

    fn open(&self, caller: &Caller, args: &Value, draft: Draft) -> Result<Value, ApiError> {
        let Draft {
            kind,
            op,
            target,
            change,
            facts,
            key,
        } = draft;
        let (reason, expires_in_ms, asked_key) = options(args);
        let digest = sha256_hex(change.to_string().as_bytes());
        let opening = Opening {
            parts: None,
            kind,
            target: target.clone(),
            facts,
            request: json!({"op": op, "action": change["action"], "target": target, "digest": digest}),
            change,
            reason,
            expires_in_ms,
            key: key.or(asked_key),
            retry_of: retry_of(args),
        };
        if dry_run(args) {
            let would = self.consent.would(&opening)?;
            let mut change = opening.change.clone();
            if let Some(map) = change.as_object_mut() {
                map.remove("message");
            }
            return Ok(json!({"dryRun": true, "op": op, "would": "ask",
                "change": change, "request": would, "error": null}));
        }
        let opened = self.consent.open(caller, opening)?;
        let row = opened.row();
        Ok(json!({"state": row.state.as_str(), "request": self.consent.project(row, true)}))
    }

    fn dispatch_facts(change: &Value, online: bool) -> Value {
        let mut lines = vec![
            json!({"line": "consent-job-dispatch", "args": {
                "tool": change["tool"], "machine": change["machine"], "folder": change["folder"],
            }}),
            json!({"line": if online {
                "consent-job-machine-online"
            } else {
                "consent-job-machine-offline"
            }, "args": {"machine": change["machine"], "organization": change["organization"]}}),
        ];
        if let Some(model) = change["model"].as_str() {
            lines.push(json!({"line": "consent-job-model", "args": {
                "model": model, "effort": change["effort"],
            }}));
        }
        if let Some(names) = change["secrets"].as_array().filter(|s| !s.is_empty()) {
            let names: Vec<&str> = names.iter().filter_map(Value::as_str).collect();
            lines.push(json!({"line": "consent-job-secrets", "args": {"names": names.join(", ")}}));
        }
        Value::Array(lines)
    }

    fn prepare(&self, caller: &Caller, args: &Value) -> Result<(Value, Member, String), ApiError> {
        let who = self.consent.who(caller);
        let (session, _) = caller_of(&who)?;
        let asked = args
            .get("machine")
            .or_else(|| args.get("node"))
            .and_then(Value::as_str)
            .filter(|m| !m.trim().is_empty())
            .ok_or_else(|| bad(DISPATCH_USAGE))?;
        let member = self
            .fleet
            .member(asked)
            .ok_or_else(|| bad("No such machine in the cluster.").with_reason("no_machine"))?;
        self.same_org(&who, &member.organization)?;
        let mut shaped = args.clone();
        shaped["node"] = json!(member.id);
        let (key, spec) = build(&shaped, OriginTier::App)?;
        if self.key_taken(&session, &job_id(&key)) {
            return Err(bad(KEY_IN_USE).with_reason("key_in_use"));
        }
        for name in &spec.secrets {
            let theirs = self
                .fleet
                .secret_organization(name)
                .ok_or_else(|| bad("No such secret.").with_reason("no_secret"))?;
            self.same_org(&who, &theirs)?;
            if theirs != member.organization {
                return Err(forbidden(OTHER_ORG, "other_organization"));
            }
        }
        let tool = args["tool"].as_str().unwrap_or_default();
        let message = match tool {
            "shell" => args["command"].as_str(),
            _ => args["text"].as_str(),
        };
        let pick = |name: &str| {
            args.get(name)
                .and_then(Value::as_str)
                .filter(|t| !t.trim().is_empty())
                .map(str::to_string)
        };
        let change = json!({
            "action": "dispatch", "tool": tool, "machine": member.name, "node": member.id,
            "folder": spec.cwd, "organization": member.organization,
            "model": pick("model"), "effort": pick("effort"), "secrets": spec.secrets,
            "key": key, "message": message,
        });
        Ok((change, member, key))
    }

    pub fn dispatch(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        self.gate()?;
        let (change, member, key) = self.prepare(caller, args)?;
        let facts = Self::dispatch_facts(&change, member.online);
        self.open(
            caller,
            args,
            Draft {
                kind: DISPATCH,
                op: ops::CLUSTER_DISPATCH,
                target: member.id,
                change,
                facts,
                key: Some(key),
            },
        )
    }

    fn row_of(&self, id: &str) -> Result<JobRow, ApiError> {
        let found = self
            .fleet
            .jobs()
            .into_iter()
            .find(|j| j.id == id)
            .ok_or_else(|| bad(NO_JOB).with_reason("no_job"))?;
        Ok(found)
    }

    fn relation_own(&self, who: &AgentIdentity, id: &str) -> bool {
        match who.session.as_deref().filter(|_| who.verified) {
            Some(session) => self.book.owned_by(session, id).is_some(),
            None => false,
        }
    }

    fn act(&self, caller: &Caller, args: &Value, act: Act) -> Result<Value, ApiError> {
        self.gate()?;
        let usage = match act {
            Act::Tail => TAIL_USAGE,
            Act::Return => RETURN_USAGE,
            Act::Stop => STOP_USAGE,
        };
        let id = text_of(args, "id", usage)?;
        let from = args.get("from").and_then(Value::as_u64).unwrap_or(0);
        let job = self.row_of(id)?;
        let who = self.consent.who(caller);
        self.same_org(&who, &job.organization)?;
        if act == Act::Stop {
            self.stoppable(&job)
                .map_err(|(text, why)| bad(&text).with_reason(why))?;
        }
        if self.relation_own(&who, &job.id) {
            if dry_run(args) {
                return Ok(json!({"dryRun": true, "op": act.op(), "would": "apply",
                    "change": Self::change_of(&job, act, from), "request": null, "error": null}));
            }
            let result = self.execute(&job, act, from)?;
            return Ok(
                json!({"free": true, "action": act.word(), "job": job.id, "result": result}),
            );
        }
        let change = Self::change_of(&job, act, from);
        let facts = json!([{"line": act.line(), "args": {
            "machine": change["machine"], "folder": change["folder"], "job": change["job"],
        }}]);
        self.open(
            caller,
            args,
            Draft {
                kind: act.kind(),
                op: act.op(),
                target: job.id.clone(),
                change,
                facts,
                key: None,
            },
        )
    }

    fn stoppable(&self, job: &JobRow) -> Result<(), (String, &'static str)> {
        if is_terminal(&job.state) {
            return Err(("That job already ended.".to_string(), "job_ended"));
        }
        if !self.hands.can_halt(job) {
            return Err((
                "Only a shell job on this Mac can be stopped yet.".to_string(),
                "not_yet",
            ));
        }
        Ok(())
    }

    fn change_of(job: &JobRow, act: Act, from: u64) -> Value {
        json!({
            "action": act.word(), "job": job.id, "machine": job.machine, "node": job.node,
            "folder": job.folder, "tool": job.tool, "organization": job.organization,
            "from": from,
        })
    }

    fn execute(&self, job: &JobRow, act: Act, from: u64) -> Result<Value, ApiError> {
        match act {
            Act::Tail => self.hands.tail(job, from),
            Act::Return => self.hands.give_back(job),
            Act::Stop => self.hands.halt(job),
        }
    }

    fn begin(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let tool = text_of(args, "tool", BEGIN_USAGE)?;
        let folder = text_of(args, "folder", BEGIN_USAGE)?;
        if !folder.starts_with('/') || folder.contains('\0') {
            return Err(bad(BEGIN_USAGE));
        }
        let who = self.consent.who(caller);
        let (session, organization) = caller_of(&who)?;
        let now = (self.clock)();
        let id = self.book.fresh_id(now);
        if dry_run(args) {
            return Ok(
                json!({"dryRun": true, "op": ops::DELEGATE_BEGIN, "would": "apply",
                "change": {"tool": tool, "folder": folder}, "request": null, "error": null}),
            );
        }
        self.book.record_with_pid(
            &id,
            Record {
                session,
                organization,
                kind: "delegate".to_string(),
                tool: tool.chars().take(40).collect(),
                folder: folder.to_string(),
                started_ms: now,
                ended_ms: None,
                exit: None,
            },
            caller.pid.filter(|pid| pid_alive(*pid)),
        );
        Ok(json!({"id": id}))
    }

    fn end(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let id = text_of(args, "id", END_USAGE)?;
        let who = self.consent.who(caller);
        if !self.relation_own(&who, id) {
            return Err(bad(NO_JOB).with_reason("no_job"));
        }
        if dry_run(args) {
            return Ok(
                json!({"dryRun": true, "op": ops::DELEGATE_END, "would": "apply",
                "change": {"id": id}, "request": null, "error": null}),
            );
        }
        let exit = args.get("exit").and_then(Value::as_i64);
        let ended = self.book.end(id, (self.clock)(), exit);
        Ok(json!({"id": id, "ended": ended}))
    }

    fn apply_row(&self, row: &Consent) -> Result<Value, Failure> {
        self.still_fits(row).map_err(Failure::from)?;
        let change = &row.change;
        if row.kind == DISPATCH {
            return self.run_dispatch(row);
        }
        let act = change["action"]
            .as_str()
            .and_then(Act::parse)
            .ok_or_else(|| Failure::from("not a job action".to_string()))?;
        let id = change["job"].as_str().unwrap_or_default();
        let job = self.row_of(id).map_err(|e| Failure::of_api(&e))?;
        self.execute(&job, act, change["from"].as_u64().unwrap_or(0))
            .map_err(|e| Failure::of_api(&e))
    }

    pub(crate) fn dispatch_args(change: &Value) -> Value {
        let tool = change["tool"].as_str().unwrap_or_default();
        let mut args = json!({
            "node": change["node"], "key": change["key"], "tool": tool, "cwd": change["folder"],
        });
        for name in ["model", "effort"] {
            if let Some(word) = change[name].as_str() {
                args[name] = json!(word);
            }
        }
        if let Some(text) = change["message"].as_str() {
            args[if tool == "shell" { "command" } else { "text" }] = json!(text);
        }
        if let Some(names) = change["secrets"].as_array().filter(|s| !s.is_empty()) {
            args["secrets"] = json!(names);
        }
        args
    }

    fn run_dispatch(&self, row: &Consent) -> Result<Value, Failure> {
        let change = &row.change;
        let args = Self::dispatch_args(change);
        let started = self
            .hands
            .dispatch(&args)
            .map_err(|e| Failure::of_api(&e))?;
        let key = change["key"].as_str().unwrap_or_default();
        let id = started["job"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| job_id(key));
        let session = row.session.clone().filter(|_| row.verified);
        let claimed = session.is_some_and(|session| {
            self.book.claim(
                &id,
                Record {
                    session,
                    organization: change["organization"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    kind: "dispatch".to_string(),
                    tool: change["tool"].as_str().unwrap_or_default().to_string(),
                    folder: change["folder"].as_str().unwrap_or_default().to_string(),
                    started_ms: (self.clock)(),
                    ended_ms: None,
                    exit: None,
                },
            )
        });
        if !claimed {
            return Err(Failure::from(
                "That key already started another session's job.".to_string(),
            ));
        }
        Ok(json!({"job": id, "state": started["state"], "warning": started["warning"]}))
    }

    fn still_fits(&self, row: &Consent) -> Result<(), String> {
        if !(self.cluster_on)() {
            return Err("Cluster is off".to_string());
        }
        let change = &row.change;
        if row.kind == DISPATCH {
            let session = row.session.as_deref().filter(|_| row.verified);
            let key = change["key"].as_str().unwrap_or_default();
            if session.is_none_or(|s| self.key_taken(s, &job_id(key))) {
                return Err("that key already started another session's job".to_string());
            }
            let node = change["node"].as_str().unwrap_or_default();
            let member = self
                .fleet
                .member(node)
                .ok_or_else(|| "the machine is gone".to_string())?;
            if Some(member.organization.as_str()) != change["organization"].as_str() {
                return Err("the machine moved to another organization".to_string());
            }
            for name in change["secrets"].as_array().into_iter().flatten() {
                let name = name.as_str().unwrap_or_default();
                if self.fleet.secret_organization(name).as_deref()
                    != Some(member.organization.as_str())
                {
                    return Err("a secret changed organization".to_string());
                }
            }
            return Ok(());
        }
        let id = change["job"].as_str().unwrap_or_default();
        let job = self.row_of(id).map_err(|_| "the job is gone".to_string())?;
        if Some(job.organization.as_str()) != change["organization"].as_str() {
            return Err("the job moved to another organization".to_string());
        }
        if row.kind == JOB_STOP {
            self.stoppable(&job).map_err(|(text, _)| text)?;
        }
        Ok(())
    }
}

struct Apply(Weak<JobAsks>);

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
        by: Option<Answerer>,
        device: Option<&str>,
    ) -> Result<Value, String> {
        self.apply_failing(row, by, device)
            .map_err(|failure| failure.message)
    }

    fn apply_failing(
        &self,
        row: &Consent,
        _by: Option<Answerer>,
        _device: Option<&str>,
    ) -> Result<Value, Failure> {
        match self.0.upgrade() {
            Some(asks) => asks.apply_row(row),
            None => Err(Failure::from("stopping".to_string())),
        }
    }

    fn slow(&self, _row: &Consent) -> bool {
        true
    }

    fn shape(&self, row: &Consent, view: &mut Value, agent: bool) {
        if agent && !row.state.open() {
            if let Some(change) = view.get_mut("change").and_then(Value::as_object_mut) {
                change.remove("message");
            }
        }
    }
}

impl OpGroup for JobAsks {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::JOBS_MINE | ops::JOBS_GET => Some(OpSpec::AGENT_READ),
            ops::JOBS_TAIL
            | ops::JOBS_RETURN
            | ops::JOBS_STOP
            | ops::DELEGATE_BEGIN
            | ops::DELEGATE_END => Some(OpSpec::AGENT_ACTS),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::JOBS_MINE => self.mine(caller),
            ops::JOBS_GET => self.get(caller, args),
            ops::JOBS_TAIL => self.act(caller, args, Act::Tail),
            ops::JOBS_RETURN => self.act(caller, args, Act::Return),
            ops::JOBS_STOP => self.act(caller, args, Act::Stop),
            ops::DELEGATE_BEGIN => self.begin(caller, args),
            ops::DELEGATE_END => self.end(caller, args),
            ops::CLUSTER_DISPATCH => self.dispatch(caller, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

#[path = "job_reads.rs"]
mod reads;

#[cfg(test)]
#[path = "job_asks_failure_tests.rs"]
mod failure_tests;

#[cfg(test)]
#[path = "job_asks_leak_tests.rs"]
mod leak_tests;

#[cfg(test)]
#[path = "job_asks_lost_tests.rs"]
mod lost_tests;

#[cfg(test)]
#[path = "job_asks_pid_tests.rs"]
mod pid_tests;

#[cfg(test)]
#[path = "job_asks_late_tests.rs"]
mod late_tests;
#[cfg(test)]
#[path = "job_asks_slow_tests.rs"]
mod slow_tests;

#[cfg(test)]
#[path = "job_asks_review_tests.rs"]
mod review_tests;

#[cfg(test)]
#[path = "job_asks_tests.rs"]
pub(crate) mod tests;
