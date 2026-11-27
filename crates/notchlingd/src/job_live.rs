use crate::cluster_dispatch::DispatchOps;
use crate::cluster_mirror::TailSource;
use crate::cluster_node::ClusterNode;
use crate::cluster_sessions::tail;
use crate::cluster_workspace_send::ReturnOps;
use crate::job_asks::{Fleet, Hands, JobRow, Member, TAIL_TEXT};
use crate::op_group::OpGroup;
use crate::shell_jobs::ShellJobs;
use notchling_engine::bridge::net::crypto::base64_decode;
use notchling_engine::cluster::NodeId;
use notchling_engine::machines::registry::{RegistryFiles, RegistryState};
use notchling_engine::organizations::{Kind, Organizations, PERSONAL_ID};
use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, PoisonError, Weak};

pub struct LiveFleet {
    node: Result<Arc<ClusterNode>, String>,
    registry: RegistryFiles,
    orgs: Organizations,
}

impl LiveFleet {
    pub fn new(
        node: Result<Arc<ClusterNode>, String>,
        registry: RegistryFiles,
        orgs: Organizations,
    ) -> LiveFleet {
        LiveFleet {
            node,
            registry,
            orgs,
        }
    }

    fn organization_of(&self, name: &str) -> String {
        match self.registry.load() {
            RegistryState::Loaded { machines, .. } => machines
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(name))
                .map(|m| m.organization_id().to_string()),
            _ => None,
        }
        .unwrap_or_else(|| PERSONAL_ID.to_string())
    }

    fn members(&self) -> Vec<Member> {
        let Ok(node) = &self.node else {
            return Vec::new();
        };
        let me = node.me().as_str().to_string();
        let status = node.status();
        status
            .get("members")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let id = m.get("id").and_then(Value::as_str)?.to_string();
                let name = m.get("name").and_then(Value::as_str)?.to_string();
                Some(Member {
                    organization: self.organization_of(&name),
                    online: m.get("online").and_then(Value::as_bool) == Some(true),
                    me: id == me,
                    id,
                    name,
                })
            })
            .collect()
    }
}

impl Fleet for LiveFleet {
    fn member(&self, name_or_id: &str) -> Option<Member> {
        let members = self.members();
        members
            .iter()
            .find(|m| m.id == name_or_id)
            .or_else(|| {
                members
                    .iter()
                    .find(|m| m.name.eq_ignore_ascii_case(name_or_id))
            })
            .cloned()
    }

    fn jobs(&self) -> Vec<JobRow> {
        let Ok(node) = &self.node else {
            return Vec::new();
        };
        let Some(state) = node.state() else {
            return Vec::new();
        };
        let members = self.members();
        state
            .jobs
            .values()
            .map(|job| {
                let member = members.iter().find(|m| m.id == job.spec.node.as_str());
                let word = |v: Value| v.as_str().unwrap_or_default().to_string();
                JobRow {
                    id: job.id.clone(),
                    node: job.spec.node.as_str().to_string(),
                    machine: member.map(|m| m.name.clone()).unwrap_or_default(),
                    organization: member
                        .map(|m| m.organization.clone())
                        .unwrap_or_else(|| PERSONAL_ID.to_string()),
                    tool: word(json!(job.spec.tool)),
                    folder: job.spec.cwd.clone(),
                    state: word(json!(job.state)),
                    created_ms: job.created_ms,
                    session: job.session.clone(),
                    detail: job.detail.clone(),
                    secrets: job.spec.secrets.clone(),
                    local: member.is_some_and(|m| m.me),
                }
            })
            .collect()
    }

    fn secret_organization(&self, name: &str) -> Option<String> {
        let wanted = Kind::Vault.normal(name);
        self.orgs
            .listing(Kind::Vault)?
            .iter()
            .any(|(key, _)| *key == wanted)
            .then(|| self.orgs.resolve(Kind::Vault, name).id)
    }
}

#[cfg(test)]
#[path = "job_live_tests.rs"]
mod tests;

fn the_app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

fn failed(text: &str) -> ApiError {
    ApiError::new(ErrorCode::Failed, text)
}

pub struct LiveHands {
    dispatch: Weak<DispatchOps>,
    returns: Arc<ReturnOps>,
    shells: Arc<ShellJobs>,
    sessions: Arc<Mutex<Value>>,
    tails: Arc<dyn TailSource>,
}

impl LiveHands {
    pub fn new(
        dispatch: &Arc<DispatchOps>,
        returns: Arc<ReturnOps>,
        shells: Arc<ShellJobs>,
        sessions: Arc<Mutex<Value>>,
        tails: Arc<dyn TailSource>,
    ) -> LiveHands {
        LiveHands {
            dispatch: Arc::downgrade(dispatch),
            returns,
            shells,
            sessions,
            tails,
        }
    }

    fn transcript_id(job: &JobRow) -> Result<String, ApiError> {
        if job.tool == "shell" {
            return Ok(format!("shell:{}", job.id));
        }
        job.session
            .clone()
            .ok_or_else(|| failed("That job has no output yet."))
    }

    fn local_rows(&self) -> Vec<Value> {
        let mut rows = self.shells.rows();
        let list = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(list) = list.as_array() {
            rows.extend(list.iter().cloned());
        }
        rows
    }
}

impl Hands for LiveHands {
    fn dispatch(&self, args: &Value) -> Result<Value, ApiError> {
        let dispatch = self
            .dispatch
            .upgrade()
            .ok_or_else(|| failed("Notchling is stopping."))?;
        dispatch.call(&the_app(), ops::CLUSTER_DISPATCH, args)
    }

    fn tail(&self, job: &JobRow, from: u64) -> Result<Value, ApiError> {
        let id = Self::transcript_id(job)?;
        let got = if job.local {
            tail(&self.local_rows(), &json!({"id": id, "from": from}))?
        } else {
            let node = NodeId::parse(&job.node).ok_or_else(|| failed("No such machine."))?;
            self.tails.tail(&node, &id, from, None)?
        };
        let bytes = got
            .get("data")
            .and_then(Value::as_str)
            .and_then(base64_decode)
            .unwrap_or_default();
        let start = bytes.len().saturating_sub(TAIL_TEXT);
        let text = String::from_utf8_lossy(&bytes[start..]).into_owned();
        Ok(json!({
            "text": self.shells.mask_text(&text),
            "next": got["next"],
            "size": got["size"],
        }))
    }

    fn give_back(&self, job: &JobRow) -> Result<Value, ApiError> {
        self.returns
            .call(&the_app(), ops::CLUSTER_RETURN, &json!({"job": job.id}))
    }

    fn can_halt(&self, job: &JobRow) -> bool {
        job.tool == "shell" && job.local
    }

    fn halt(&self, job: &JobRow) -> Result<Value, ApiError> {
        if !self.can_halt(job) {
            return Err(
                failed("Only a shell job on this Mac can be stopped yet.").with_reason("not_yet")
            );
        }
        self.shells.stop(&json!({"id": job.id}))
    }
}
