use crate::vault_ops::{Looked, ReleaseFor, Released, Still};
use notchling_engine::cluster::grants::{decide, expires_ms, GrantAsk, GrantDecision};
use notchling_engine::cluster::wire::Origin;
use notchling_engine::cluster::{ClusterState, Job, Member, NodeId, Tool};
use notchling_engine::cluster::{JobState, MemberState};
use notchling_engine::vault::valid_name;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::sync::Arc;

pub const GRANT_DELIVER: &str = "grant.deliver";
pub const GRANT_REVOKE: &str = "grant.revoke";
pub const GRANT_REQUEST: &str = "grant.request";
pub const GRANT_ATTACH: &str = notchling_engine::cluster::core::GRANT_ATTACH;
pub const GRANT_REVOKE_JOB: &str = notchling_engine::cluster::core::GRANT_REVOKE_JOB;
pub const NODE_OPS: [&str; 5] = [
    GRANT_DELIVER,
    GRANT_REVOKE,
    GRANT_REQUEST,
    GRANT_ATTACH,
    GRANT_REVOKE_JOB,
];

pub const MAX_NAMES: usize = 16;
const COMMAND_CUT: usize = 120;
const TEXT_MAX: usize = 4096;
pub const LOG_DOWN: &str = "the audit log can't be written";

pub trait Vault: Send + Sync {
    fn lookup(&self, names: &[String]) -> Result<Vec<Looked>, ApiError>;
    fn release(
        &self,
        who: &ReleaseFor,
        names: &[String],
        still: &Still<'_>,
    ) -> Result<Released, ApiError>;
}

pub trait Link: Send + Sync {
    fn call(&self, node: &NodeId, op: &str, args: Value) -> Result<Value, ApiError>;
}

pub struct Release<'a> {
    pub node: &'a NodeId,
    pub who: &'a ReleaseFor,
    pub name: &'a str,
    pub released: &'a Released,
}

pub trait Audit: Send + Sync {
    fn released(&self, release: &Release<'_>) -> Result<(), String>;
    fn refused(&self, node: &NodeId, who: &ReleaseFor, name: &str);
}

pub trait Notices: Send + Sync {
    fn waiting(&self, job: &Job, node_name: &str, vault_name: &str);
    fn clear(&self, job: &str);
}

pub trait View: Send + Sync {
    fn state(&self) -> Option<ClusterState>;
    fn me(&self) -> Option<NodeId>;
}

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Grant {
    Delivered(Vec<String>),
    Waiting(String),
}

#[derive(Clone, Debug)]
pub struct Facts {
    pub member: Member,
    pub vault_name: String,
    pub vault_is_primary: bool,
}

pub struct GrantBroker {
    vault: Arc<dyn Vault>,
    link: Arc<dyn Link>,
    audit: Arc<dyn Audit>,
    notices: Arc<dyn Notices>,
    view: Arc<dyn View>,
    clock: Clock,
}

struct AttachArgs {
    job: Job,
    member: Member,
    vault_name: String,
    vault_primary: bool,
}

struct RevokeArgs {
    job: String,
    node: NodeId,
}

fn attach_args(args: &Value) -> Option<AttachArgs> {
    Some(AttachArgs {
        job: serde_json::from_value(args.get("job")?.clone()).ok()?,
        member: serde_json::from_value(args.get("member")?.clone()).ok()?,
        vault_name: args.get("vaultName")?.as_str()?.to_string(),
        vault_primary: args.get("vaultPrimary")?.as_bool()?,
    })
}

fn revoke_args(args: &Value) -> Option<RevokeArgs> {
    Some(RevokeArgs {
        job: args.get("job")?.as_str()?.to_string(),
        node: NodeId::parse(args.get("node")?.as_str()?)?,
    })
}

fn refused(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, message)
}

fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

fn canonical(names: &[String]) -> Result<Vec<String>, ApiError> {
    let mut seen: Vec<String> = Vec::new();
    for name in names {
        let lower = name.to_lowercase();
        if !seen.contains(&lower) {
            seen.push(lower);
        }
    }
    if seen.is_empty() || seen.len() > MAX_NAMES {
        return Err(ApiError::bad_request(format!(
            "ask for 1 to {MAX_NAMES} secrets"
        )));
    }
    Ok(seen)
}

fn audited(name: &str) -> &str {
    if valid_name(name) {
        name
    } else {
        "<invalid name>"
    }
}

fn command_of(job: &Job) -> String {
    let text = match job.spec.args.get("command").and_then(Value::as_str) {
        Some(command) => command.to_string(),
        None => match job.spec.tool {
            Tool::Claude => "a Claude Code session".to_string(),
            Tool::Agy => "an Antigravity session".to_string(),
            _ => "a job".to_string(),
        },
    };
    text.chars().take(COMMAND_CUT).collect()
}

impl GrantBroker {
    pub fn new(
        parts: (Arc<dyn Vault>, Arc<dyn Link>, Arc<dyn Audit>),
        notices: Arc<dyn Notices>,
        view: Arc<dyn View>,
        clock: Clock,
    ) -> GrantBroker {
        GrantBroker {
            vault: parts.0,
            link: parts.1,
            audit: parts.2,
            notices,
            view,
            clock,
        }
    }

    pub fn me(&self) -> Option<NodeId> {
        self.view.me()
    }

    pub fn grant(
        &self,
        job: &Job,
        facts: &Facts,
        names: &[String],
        who: &ReleaseFor,
    ) -> Result<Grant, ApiError> {
        let names = canonical(names)?;
        let ask = |name: &str, looked: Option<&Looked>| {
            decide(&GrantAsk {
                node: &facts.member,
                job,
                name,
                exists: looked.is_none_or(|l| l.exists),
                asks: looked.is_some_and(|l| l.asks),
                vault_is_primary: facts.vault_is_primary,
                vault_name: &facts.vault_name,
            })
        };
        let no = |name: &str, why: String| {
            self.audit.refused(&job.spec.node, who, audited(name));
            refused(why)
        };
        let mut wait: Option<String> = None;
        for name in &names {
            match ask(name, None) {
                GrantDecision::Refuse(why) => return Err(no(name, why)),
                GrantDecision::Wait(why) => wait = wait.or(Some(why)),
                GrantDecision::Release | GrantDecision::Ask => {}
            }
        }
        if let Some(why) = wait {
            return Ok(Grant::Waiting(why));
        }
        let looked = self.vault.lookup(&names)?;
        if looked.len() != names.len() {
            return Err(failed("the vault answered for the wrong names"));
        }
        for (name, found) in names.iter().zip(&looked) {
            match ask(name, Some(found)) {
                GrantDecision::Refuse(why) => return Err(no(name, why)),
                GrantDecision::Wait(why) => return Ok(Grant::Waiting(why)),
                GrantDecision::Release | GrantDecision::Ask => {}
            }
        }
        let still = || self.still(&job.id, &names);
        let released = self.vault.release(who, &names, &still)?;
        let expires = expires_ms((self.clock)());
        let changed = |why: String| {
            for name in &names {
                self.audit.refused(&job.spec.node, who, name);
            }
            refused(why)
        };
        still().map_err(changed)?;
        for (entry, _) in &released.values {
            let name = entry.id();
            let release = Release {
                node: &job.spec.node,
                who,
                name: &name,
                released: &released,
            };
            self.audit
                .released(&release)
                .map_err(|_| failed(LOG_DOWN))?;
        }
        let mut delivered = Vec::new();
        for (entry, value) in &released.values {
            let name = entry.id();
            if let Err(why) = still() {
                if !delivered.is_empty() {
                    let _ = self.revoke_job(&job.id, &job.spec.node);
                }
                return Err(changed(why));
            }
            let args = json!({
                "job": job.id,
                "name": name,
                "value": value.expose(),
                "expiresMs": expires,
            });
            if let Err(e) = self.link.call(&job.spec.node, GRANT_DELIVER, args) {
                let _ = self.revoke_job(&job.id, &job.spec.node);
                return Err(e);
            }
            delivered.push(name);
        }
        Ok(Grant::Delivered(delivered))
    }

    fn still(&self, job: &str, names: &[String]) -> Result<(), String> {
        let state = self.view.state().ok_or("this node is in no cluster")?;
        let me = self.view.me().ok_or("this node is in no cluster")?;
        if state.vault != me || state.home != me {
            return Err("this machine isn't the primary vault node now".to_string());
        }
        let found = state.jobs.get(job).ok_or("the job is gone")?;
        if !matches!(found.state, JobState::Running | JobState::Dispatching) {
            return Err(format!("job {job} isn't running"));
        }
        let member = state
            .members
            .get(&found.spec.node)
            .filter(|m| m.state == MemberState::Active)
            .ok_or("that machine isn't an active member")?;
        match names.iter().find(|n| !member.secrets.allows(n)) {
            Some(_) => Err(format!("a secret isn't allowed on {} now", member.name)),
            None => Ok(()),
        }
    }

    pub fn attach(&self, args: &Value) -> Result<Value, ApiError> {
        let a = attach_args(args).ok_or_else(|| ApiError::bad_request("not an attach request"))?;
        let facts = Facts {
            member: a.member,
            vault_name: a.vault_name,
            vault_is_primary: a.vault_primary,
        };
        let who = ReleaseFor {
            node_name: facts.member.name.clone(),
            job: a.job.id.clone(),
            command: command_of(&a.job),
            folder: a.job.spec.cwd.clone(),
        };
        match self.grant(&a.job, &facts, &a.job.spec.secrets, &who)? {
            Grant::Delivered(names) => {
                self.notices.clear(&a.job.id);
                Ok(json!({"delivered": names}))
            }
            Grant::Waiting(why) => {
                self.notices
                    .waiting(&a.job, &facts.member.name, &facts.vault_name);
                Ok(json!({"waiting": why}))
            }
        }
    }

    pub fn request(&self, origin: &Origin, args: &Value) -> Result<Value, ApiError> {
        let usage = "grant.request takes {job, names, command, folder}";
        let state = self
            .view
            .state()
            .ok_or_else(|| failed("this node is in no cluster"))?;
        if self.view.me().as_ref() != Some(&state.vault) {
            return Err(refused("this node doesn't hold the vault"));
        }
        let job_id = args
            .get("job")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request(usage))?;
        let names: Vec<String> = args
            .get("names")
            .and_then(Value::as_array)
            .ok_or_else(|| ApiError::bad_request(usage))?
            .iter()
            .map(|n| n.as_str().map(str::to_string))
            .collect::<Option<_>>()
            .ok_or_else(|| ApiError::bad_request(usage))?;
        let text = |key: &str| {
            args.get(key)
                .and_then(Value::as_str)
                .map(|t| t.chars().take(TEXT_MAX).collect::<String>())
                .unwrap_or_default()
        };
        let job = state
            .jobs
            .get(job_id)
            .ok_or_else(|| failed(format!("no job {job_id}")))?;
        if job.spec.node != origin.node {
            return Err(refused("that job isn't on your machine"));
        }
        let member = state
            .members
            .get(&origin.node)
            .ok_or_else(|| refused("not a member"))?;
        let facts = Facts {
            member: member.clone(),
            vault_name: state
                .members
                .get(&state.vault)
                .map(|m| m.name.clone())
                .unwrap_or_else(|| state.vault.to_string()),
            vault_is_primary: state.home == state.vault,
        };
        let who = ReleaseFor {
            node_name: member.name.clone(),
            job: job.id.clone(),
            command: text("command").chars().take(COMMAND_CUT).collect(),
            folder: text("folder"),
        };
        match self.grant(job, &facts, &names, &who)? {
            Grant::Delivered(names) => Ok(json!({"delivered": names})),
            Grant::Waiting(why) => Ok(json!({"waiting": why})),
        }
    }

    pub fn revoke_job(&self, job: &str, node: &NodeId) -> Result<(), ApiError> {
        self.link
            .call(node, GRANT_REVOKE, json!({"job": job}))
            .map(|_| ())
    }

    pub fn revoke(&self, args: &Value) -> Result<Value, ApiError> {
        let a = revoke_args(args).ok_or_else(|| ApiError::bad_request("not a revoke request"))?;
        self.notices.clear(&a.job);
        let _ = self.revoke_job(&a.job, &a.node);
        Ok(json!({}))
    }
}

#[cfg(test)]
#[path = "grant_broker_tests.rs"]
mod tests;
