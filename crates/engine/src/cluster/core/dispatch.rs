use serde_json::{json, Value};

use super::{
    Effect, NodeCore, Target, Then, CALL_MS, DELEGATE_CALL_MS, GRANT_ATTACH, GRANT_REVOKE_JOB,
};
use crate::cluster::grants::attachable;
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::{job_id, Job, JobSpec, JobState};
use crate::cluster::members::MemberState;
use crate::cluster::shell_job::CODEX_SECRETS;
use crate::cluster::state::Command;
use crate::cluster::text;
use crate::cluster::versions::{compat, Compat, Purpose, Versions};
use crate::cluster::wire::{Origin, OriginTier, WireError};

#[derive(serde::Deserialize)]
struct DispatchArgs {
    key: String,
    spec: JobSpec,
}

fn refusal(code: &str, message: impl Into<String>) -> WireError {
    WireError::new(code, message)
}

impl NodeCore {
    pub(super) fn reference_versions(&self) -> Versions {
        self.state
            .as_ref()
            .and_then(|s| {
                s.members
                    .values()
                    .find(|m| m.reference && m.state == MemberState::Active)
                    .and_then(|m| m.versions.clone())
            })
            .unwrap_or_else(|| self.versions.clone())
    }

    pub(super) fn dispatch_here(
        &mut self,
        target: Target,
        key: String,
        spec: JobSpec,
        tier: OriginTier,
        out: &mut Vec<Effect>,
    ) {
        if !self.is_home() {
            let err = match &self.state {
                Some(state) => {
                    let name = self.member_name(&state.home);
                    refusal("forbidden", format!("{name} is primary now"))
                }
                None => refusal("no_cluster", "this node is in no cluster"),
            };
            self.finish(target, Err(err), out);
            return;
        }
        if let Err(e) = self.fenced() {
            self.finish(target, Err(e), out);
            return;
        }
        if let Err(e) = check_spec(&spec) {
            self.finish(target, Err(e), out);
            return;
        }
        if tier != OriginTier::App && !spec.is_delegate() {
            let text = "only the Notchling app starts that on another node";
            self.finish(target, Err(refusal("forbidden", text)), out);
            return;
        }
        if let Some(job) = self.state.as_ref().and_then(|s| s.job_by_key(&key)) {
            let reply = job_reply(job, None);
            self.finish(target, Ok(reply), out);
            return;
        }
        let warning = match self.precheck(&spec, tier) {
            Ok(warning) => warning,
            Err(e) => {
                self.finish(target, Err(e), out);
                return;
            }
        };
        if let Err(e) = self.check_secrets(&spec) {
            self.finish(target, Err(e), out);
            return;
        }
        let job = job_id(&key);
        let has_secrets = !spec.secrets.is_empty();
        if let Err(e) = self.apply(Command::Dispatch { key, spec }, out) {
            self.finish(target, Err(refusal("refused", e.to_string())), out);
            return;
        }
        if has_secrets {
            self.attach(target, job, warning, out);
        } else {
            self.start_tool(target, job, warning, out);
        }
    }

    fn precheck(&self, spec: &JobSpec, tier: OriginTier) -> Result<Option<String>, WireError> {
        let node = spec.node.clone();
        let name = self.member_name(&node);
        let member_state = self
            .state
            .as_ref()
            .and_then(|s| s.members.get(&node))
            .map(|m| (m.state, m.versions.clone(), m.accepts_delegation));
        let versions = match member_state {
            None => return Err(refusal("unknown_node", format!("{name} is not a member"))),
            Some((MemberState::Removed, _, _)) => {
                return Err(refusal("removed", format!("{name} was removed")))
            }
            Some((MemberState::Pending, _, _)) => {
                return Err(refusal("pending", format!("{name} is not approved yet")))
            }
            Some((MemberState::Active, versions, accepts)) => {
                if tier != OriginTier::App && !accepts {
                    return Err(refusal(
                        "refused",
                        format!("{name} doesn't take delegated jobs"),
                    ));
                }
                if node == self.me {
                    Some(self.versions.clone())
                } else {
                    versions
                }
            }
        };
        if node != self.me && !self.is_online(&node) {
            return Err(refusal("offline", format!("{name} is offline")));
        }
        let Some(versions) = versions else {
            return Err(refusal(
                "refused",
                format!("{name} hasn't reported its versions"),
            ));
        };
        match compat(
            &self.reference_versions(),
            &versions,
            &name,
            Purpose::Dispatch(spec.tool),
        ) {
            Compat::Refuse(text) => Err(refusal("refused", text)),
            Compat::Warn(text) => Ok(Some(text)),
            Compat::Ok => Ok(None),
        }
    }

    fn check_secrets(&self, spec: &JobSpec) -> Result<(), WireError> {
        if spec.secrets.is_empty() {
            return Ok(());
        }
        let Some(state) = &self.state else {
            return Err(refusal("no_cluster", "this node is in no cluster"));
        };
        if spec.is_delegate() {
            return Err(refusal("bad_args", "a delegate job takes no secrets"));
        }
        if spec.secrets.len() > MAX_ATTACHED {
            return Err(refusal(
                "bad_args",
                format!("at most {MAX_ATTACHED} secrets go with one job"),
            ));
        }
        if spec.node == state.vault {
            let name = self.member_name(&state.vault);
            let text = format!("{name} holds the vault: a job there asks with notchling secret");
            return Err(refusal("refused", text));
        }
        if state.home != state.vault {
            let name = self.member_name(&state.vault);
            let text = format!("{name} holds the vault; {DISPATCH_AGAIN}");
            return Err(refusal("refused", text));
        }
        let allow = state
            .members
            .get(&spec.node)
            .map(|m| m.secrets.clone())
            .unwrap_or_default();
        attachable(spec, &allow).map_err(|text| refusal("refused", text))
    }

    fn start_tool(
        &mut self,
        target: Target,
        job: String,
        warning: Option<String>,
        out: &mut Vec<Effect>,
    ) {
        let Some(spec) = self
            .state
            .as_ref()
            .and_then(|s| s.jobs.get(&job))
            .map(|j| j.spec.clone())
        else {
            self.finish(target, Err(refusal("refused", "no such job")), out);
            return;
        };
        let node = spec.node.clone();
        let (op, origin, wait) = if spec.is_delegate() {
            let origin = Origin {
                node: self.me.clone(),
                tier: OriginTier::Agent,
            };
            ("delegate.run", origin, DELEGATE_CALL_MS)
        } else {
            let origin = Origin {
                node: self.me.clone(),
                tier: OriginTier::App,
            };
            (Self::tool_op(spec.tool), origin, CALL_MS)
        };
        let args = start_args(&spec, &job);
        let then = Then::Dispatch {
            job,
            warning,
            target,
        };
        if node == self.me {
            let call = self.next_local;
            self.next_local += 1;
            self.local_runs.insert(call, then);
            let term = self.current_term();
            out.push(Effect::RunLocal {
                call,
                op: op.to_string(),
                args: args.into(),
                origin,
                term,
            });
        } else {
            self.call_out_within(&node, op.to_string(), args, origin, (then, wait), out);
        }
    }

    fn attach(
        &mut self,
        target: Target,
        job: String,
        warning: Option<String>,
        out: &mut Vec<Effect>,
    ) {
        let Some(args) = self.attach_args(&job) else {
            self.fail_job(&job, "no such job", out);
            self.finish(target, Err(refusal("refused", "no such job")), out);
            return;
        };
        let call = self.next_local;
        self.next_local += 1;
        self.local_runs.insert(
            call,
            Then::Attach {
                job,
                warning,
                target,
            },
        );
        let origin = self.node_origin();
        let term = self.current_term();
        out.push(Effect::RunLocal {
            call,
            op: GRANT_ATTACH.to_string(),
            args: args.into(),
            origin,
            term,
        });
    }

    fn attach_args(&self, job: &str) -> Option<Value> {
        let state = self.state.as_ref()?;
        let found = state.jobs.get(job)?;
        let member = state.members.get(&found.spec.node)?;
        Some(json!({
            "job": found,
            "member": member,
            "vaultName": self.member_name(&state.vault),
            "vaultPrimary": state.home == state.vault && state.home == self.me,
        }))
    }

    pub(super) fn attached(
        &mut self,
        job: String,
        warning: Option<String>,
        target: Target,
        result: Result<Value, WireError>,
        out: &mut Vec<Effect>,
    ) {
        if !self.is_home() {
            self.revoke_grants(&job, out);
            let text = "this machine is no longer the primary";
            self.finish(target, Err(refusal("refused", text)), out);
            return;
        }
        let current = self
            .state
            .as_ref()
            .and_then(|s| s.jobs.get(&job))
            .map(|j| j.state);
        if current != Some(JobState::Dispatching) {
            self.revoke_grants(&job, out);
            let text = "the job ended before its secrets arrived";
            self.finish(target, Err(refusal("refused", text)), out);
            return;
        }
        let waiting = result
            .as_ref()
            .ok()
            .and_then(|v| v.get("waiting"))
            .and_then(Value::as_str)
            .map(str::to_string);
        match (result, waiting) {
            (Err(e), _) => {
                self.fail_job(&job, &e.message, out);
                self.finish(target, Err(e), out);
            }
            (Ok(_), Some(text)) => {
                let why = format!("{text}; {DISPATCH_AGAIN}");
                self.fail_job(&job, &why, out);
                self.finish(target, Err(refusal("refused", why)), out);
            }
            (Ok(_), None) => self.start_tool(target, job, warning, out),
        }
    }

    fn fail_job(&mut self, job: &str, why: &str, out: &mut Vec<Effect>) {
        let command = Command::JobState {
            id: job.to_string(),
            state: JobState::Failed,
            session: None,
            detail: Some(why.chars().take(DETAIL_MAX).collect()),
        };
        let _ = self.apply(command, out);
        self.revoke_grants(job, out);
    }

    pub(super) fn revoke_grants(&mut self, job: &str, out: &mut Vec<Effect>) {
        let Some(found) = self.state.as_ref().and_then(|s| s.jobs.get(job)) else {
            return;
        };
        let args = json!({"job": job, "node": found.spec.node});
        let call = self.next_local;
        self.next_local += 1;
        self.local_runs.insert(call, Then::Reply(Target::Nobody));
        let origin = self.node_origin();
        let term = self.current_term();
        out.push(Effect::RunLocal {
            call,
            op: GRANT_REVOKE_JOB.to_string(),
            args: args.into(),
            origin,
            term,
        });
    }

    pub(super) fn native_dispatch(
        &mut self,
        target: Target,
        args: &Value,
        origin: &Origin,
        out: &mut Vec<Effect>,
    ) {
        match serde_json::from_value::<DispatchArgs>(args.clone()) {
            Ok(a) => self.dispatch_here(target, a.key, a.spec, origin.tier, out),
            Err(_) => self.finish(target, Err(refusal("bad_args", "not a dispatch")), out),
        }
    }

    pub(super) fn on_job_event(
        &mut self,
        from: &NodeId,
        name: &str,
        data: &Value,
        out: &mut Vec<Effect>,
    ) {
        if name != "job.state" || !self.is_home() {
            return;
        }
        let Some(job) = data.get("job").and_then(Value::as_str) else {
            return;
        };
        let Some(state) = data
            .get("state")
            .and_then(|s| serde_json::from_value::<JobState>(s.clone()).ok())
        else {
            return;
        };
        let Some(found) = self.state.as_ref().and_then(|s| s.jobs.get(job)) else {
            return;
        };
        if found.spec.node != *from {
            return;
        }
        if state == JobState::Running {
            let session = data
                .get("session")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(|s| text::clean(s, SESSION_MAX));
            if found.state == JobState::Running && found.session.is_none() && session.is_some() {
                let command = Command::JobState {
                    id: job.to_string(),
                    state,
                    session,
                    detail: None,
                };
                let _ = self.apply(command, out);
            }
            return;
        }
        if !matches!(
            state,
            JobState::Done | JobState::Failed | JobState::Interrupted
        ) || !found.state.may_become(state)
        {
            return;
        }
        let detail = data
            .get("detail")
            .and_then(Value::as_str)
            .map(|d| text::clean(d, DETAIL_MAX));
        let command = Command::JobState {
            id: job.to_string(),
            state,
            session: None,
            detail,
        };
        let _ = self.apply(command, out);
        self.revoke_grants(job, out);
    }

    pub(super) fn dispatched(
        &mut self,
        job: String,
        warning: Option<String>,
        result: Result<Value, WireError>,
        out: &mut Vec<Effect>,
    ) -> Result<Value, WireError> {
        let found = self.state.as_ref().and_then(|s| s.jobs.get(&job));
        let delegate = found.is_some_and(|j| j.spec.is_delegate());
        let found_tool = found.map(|j| j.spec.tool);
        let node = found.map(|j| j.spec.node.clone());
        let current = found.map(|j| j.state);
        let has_session = found.is_some_and(|j| j.session.is_some());
        let reply = match result {
            Ok(reply) => reply,
            Err(e) => {
                let command = Command::JobState {
                    id: job.clone(),
                    state: JobState::Failed,
                    session: None,
                    detail: Some(text::clean(&e.message, DETAIL_MAX)),
                };
                let _ = self.apply(command, out);
                self.revoke_grants(&job, out);
                return Err(e);
            }
        };
        let codex = found_tool == Some(Tool::Codex);
        let first = if codex { "id" } else { "session" };
        let second = if codex { "session" } else { "id" };
        let session = reply
            .get(first)
            .or_else(|| reply.get(second))
            .or_else(|| reply.get("jobId"))
            .and_then(|s| s.as_str())
            .map(|s| text::clean(s, SESSION_MAX));
        if current == Some(JobState::Dispatching) {
            let command = Command::JobState {
                id: job.clone(),
                state: JobState::Running,
                session: session.clone(),
                detail: warning.clone(),
            };
            let _ = self.apply(command, out);
        } else if let (Some(state), Some(session), false) = (current, &session, has_session) {
            let command = Command::JobState {
                id: job.clone(),
                state,
                session: Some(session.clone()),
                detail: None,
            };
            let _ = self.apply(command, out);
        }
        if delegate {
            let exit = reply.get("exit").and_then(Value::as_i64).unwrap_or(1);
            let (state, detail) = if exit == 0 {
                (JobState::Done, warning.clone())
            } else {
                (JobState::Failed, Some(format!("exit {exit}")))
            };
            let command = Command::JobState {
                id: job.clone(),
                state,
                session: None,
                detail,
            };
            let _ = self.apply(command, out);
            self.revoke_grants(&job, out);
        }
        let state = self
            .state
            .as_ref()
            .and_then(|s| s.jobs.get(&job))
            .map(|j| j.state)
            .unwrap_or(JobState::Running);
        let qualified = match (&node, &session) {
            (Some(n), Some(s)) => Some(crate::cluster::id::qualify(n, s)),
            _ => None,
        };
        let mut body = json!({
            "job": job,
            "state": state,
            "session": session,
            "qualified": qualified,
            "warning": warning,
        });
        if delegate {
            for field in ["exit", "out", "err"] {
                if let Some(v) = reply.get(field) {
                    body[field] = v.clone();
                }
            }
        }
        Ok(body)
    }
}

const DETAIL_MAX: usize = 200;
const SESSION_MAX: usize = 200;
const MAX_ATTACHED: usize = 16;
const DISPATCH_AGAIN: &str = "dispatch again when it is primary";

fn job_reply(job: &Job, warning: Option<&str>) -> Value {
    json!({
        "job": job.id,
        "state": job.state,
        "session": job.session,
        "warning": warning.or(job.detail.as_deref()),
    })
}

fn check_spec(spec: &JobSpec) -> Result<(), WireError> {
    if spec.args.get("delegate").is_some() && !(spec.is_delegate() && spec.tool == Tool::Agy) {
        return Err(refusal(
            "bad_args",
            "a delegate job runs on Antigravity only",
        ));
    }
    if spec.tool == Tool::Codex && !spec.secrets.is_empty() {
        return Err(refusal("refused", CODEX_SECRETS));
    }
    Ok(())
}

fn start_args(spec: &JobSpec, job: &str) -> Value {
    if let Some(list) = spec.args.get("delegate").filter(|d| d.is_array()) {
        return json!({"args": list, "job": job});
    }
    let mut args = match &spec.args {
        Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    };
    let key = match spec.tool {
        Tool::Codex | Tool::Agy => "draft_folder",
        Tool::Claude => "folder",
        Tool::Shell => "cwd",
    };
    args.insert(key.to_string(), json!(spec.cwd));
    args.insert("job".to_string(), json!(job));
    Value::Object(args)
}
