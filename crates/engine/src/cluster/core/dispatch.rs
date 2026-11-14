use serde_json::{json, Value};

use super::{Effect, NodeCore, Target, Then};
use crate::cluster::jobs::{job_id, Job, JobSpec};
use crate::cluster::members::MemberState;
use crate::cluster::state::Command;
use crate::cluster::versions::{compat, Compat, Purpose, Versions};
use crate::cluster::wire::{Origin, WireError};

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
        out: &mut Vec<Effect>,
    ) {
        if !self.is_home() {
            self.finish(target, Err(refusal("forbidden", "not the primary")), out);
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
            .map(|m| (m.state, m.versions.clone()));
        let versions = match member_state {
            None => {
                self.finish(
                    target,
                    Err(refusal("unknown_node", format!("{name} is not a member"))),
                    out,
                );
                return;
            }
            Some((MemberState::Removed, _)) => {
                self.finish(
                    target,
                    Err(refusal("removed", format!("{name} was removed"))),
                    out,
                );
                return;
            }
            Some((MemberState::Pending, _)) => {
                self.finish(
                    target,
                    Err(refusal("pending", format!("{name} is not approved yet"))),
                    out,
                );
                return;
            }
            Some((MemberState::Active, versions)) => {
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
        let op = Self::tool_op(spec.tool);
        let args = start_args(&spec, &job);
        let then = Then::Dispatch {
            job,
            warning,
            target,
        };
        let origin = self.node_origin();
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
            self.call_out(&node, op.to_string(), args, origin, then, out);
        }
    }

    pub(super) fn native_dispatch(
        &mut self,
        target: Target,
        args: &Value,
        _origin: &Origin,
        out: &mut Vec<Effect>,
    ) {
        match serde_json::from_value::<DispatchArgs>(args.clone()) {
            Ok(a) => self.dispatch_here(target, a.key, a.spec, out),
            Err(_) => self.finish(target, Err(refusal("bad_args", "not a dispatch")), out),
        }
    }
}

fn job_reply(job: &Job, warning: Option<&str>) -> Value {
    json!({
        "job": job.id,
        "state": job.state,
        "session": job.session,
        "warning": warning.or(job.detail.as_deref()),
    })
}

fn start_args(spec: &JobSpec, job: &str) -> Value {
    let mut args = match &spec.args {
        Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    };
    let key = match spec.tool {
        crate::cluster::id::Tool::Codex => "draft_folder",
        _ => "cwd",
    };
    args.insert(key.to_string(), json!(spec.cwd));
    args.insert("job".to_string(), json!(job));
    Value::Object(args)
}
