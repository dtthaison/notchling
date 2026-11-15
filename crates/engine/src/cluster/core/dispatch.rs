use serde_json::{json, Value};

use super::{Effect, NodeCore, Target, Then, CALL_MS, DELEGATE_CALL_MS};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::{job_id, Job, JobSpec, JobState};
use crate::cluster::members::MemberState;
use crate::cluster::shell_job::{CODEX_LATER, SECRETS_LATER};
use crate::cluster::state::Command;
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
        let node = spec.node.clone();
        let name = self.member_name(&node);
        let member_state = self
            .state
            .as_ref()
            .and_then(|s| s.members.get(&node))
            .map(|m| (m.state, m.versions.clone(), m.accepts_delegation));
        let versions = match member_state {
            None => {
                self.finish(
                    target,
                    Err(refusal("unknown_node", format!("{name} is not a member"))),
                    out,
                );
                return;
            }
            Some((MemberState::Removed, _, _)) => {
                self.finish(
                    target,
                    Err(refusal("removed", format!("{name} was removed"))),
                    out,
                );
                return;
            }
            Some((MemberState::Pending, _, _)) => {
                self.finish(
                    target,
                    Err(refusal("pending", format!("{name} is not approved yet"))),
                    out,
                );
                return;
            }
            Some((MemberState::Active, versions, accepts)) => {
                if tier != OriginTier::App && !accepts {
                    let text = format!("{name} doesn't take delegated jobs");
                    self.finish(target, Err(refusal("refused", text)), out);
                    return;
                }
                if node == self.me {
                    Some(self.versions.clone())
                } else {
                    versions
                }
            }
        };
        if node != self.me && !self.is_online(&node) {
            self.finish(
                target,
                Err(refusal("offline", format!("{name} is offline"))),
                out,
            );
            return;
        }
        let Some(versions) = versions else {
            let text = format!("{name} hasn't reported its versions");
            self.finish(target, Err(refusal("refused", text)), out);
            return;
        };
        let warning = match compat(
            &self.reference_versions(),
            &versions,
            &name,
            Purpose::Dispatch(spec.tool),
        ) {
            Compat::Refuse(text) => {
                self.finish(target, Err(refusal("refused", text)), out);
                return;
            }
            Compat::Warn(text) => Some(text),
            Compat::Ok => None,
        };
        let job = job_id(&key);
        if let Err(e) = self.apply(
            Command::Dispatch {
                key,
                spec: spec.clone(),
            },
            out,
        ) {
            self.finish(target, Err(refusal("refused", e.to_string())), out);
            return;
        }
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
                args,
                origin,
                term,
            });
        } else {
            self.call_out_within(&node, op.to_string(), args, origin, (then, wait), out);
        }
    }

    pub(super) fn native_dispatch(
        &mut self,
        target: Target,
        args: &Value,
        origin: &Origin,
        out: &mut Vec<Effect>,
    ) {
        match serde_json::from_value::<DispatchArgs>(args.clone()) {
            Ok(a) if a.spec.tool == Tool::Codex => {
                self.finish(target, Err(refusal("refused", CODEX_LATER)), out)
            }
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
        if !matches!(
            state,
            JobState::Done | JobState::Failed | JobState::Interrupted
        ) {
            return;
        }
        let Some(found) = self.state.as_ref().and_then(|s| s.jobs.get(job)) else {
            return;
        };
        if found.spec.node != *from || !found.state.may_become(state) {
            return;
        }
        let detail = data
            .get("detail")
            .and_then(Value::as_str)
            .map(|d| d.chars().take(DETAIL_MAX).collect::<String>());
        let command = Command::JobState {
            id: job.to_string(),
            state,
            session: None,
            detail,
        };
        let _ = self.apply(command, out);
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
        let node = found.map(|j| j.spec.node.clone());
        let current = found.map(|j| j.state);
        let has_session = found.is_some_and(|j| j.session.is_some());
        let reply = match result {
            Ok(reply) => reply,
            Err(e) => {
                let command = Command::JobState {
                    id: job,
                    state: JobState::Failed,
                    session: None,
                    detail: Some(e.message.clone()),
                };
                let _ = self.apply(command, out);
                return Err(e);
            }
        };
        let session = reply
            .get("session")
            .or_else(|| reply.get("id"))
            .or_else(|| reply.get("jobId"))
            .and_then(|s| s.as_str())
            .map(str::to_string);
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

fn job_reply(job: &Job, warning: Option<&str>) -> Value {
    json!({
        "job": job.id,
        "state": job.state,
        "session": job.session,
        "warning": warning.or(job.detail.as_deref()),
    })
}

fn check_spec(spec: &JobSpec) -> Result<(), WireError> {
    if !spec.secrets.is_empty() {
        return Err(refusal("refused", SECRETS_LATER));
    }
    if spec.args.get("delegate").is_some() && !(spec.is_delegate() && spec.tool == Tool::Agy) {
        return Err(refusal(
            "bad_args",
            "a delegate job runs on Antigravity only",
        ));
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
