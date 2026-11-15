use crate::cluster_node::LocalCalls;
use crate::cluster_redact::TailMasks;
use crate::grant_broker::{Clock, View, GRANT_DELIVER, GRANT_REVOKE};
use crate::grant_roots::{valid_job, valid_key, GrantMarks, JobRoots, StartTool};
use crate::shell_jobs::SESSION_PREFIX;
use crate::vault_caller::Processes;
use crate::vault_ops::WorkerUse;
use notchling_engine::cluster::grant_files::{GrantFile, GrantStore, GrantValue};
use notchling_engine::cluster::grants::{decode_env, GRANT_TTL_MS};
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{ClusterState, JobState, NodeId, Tool};
use notchling_engine::vault::{hide_forms, valid_name};
use notchling_local_api::{ops, ApiError, Caller, ErrorCode};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const NOT_A_JOB: &str = "secret isn't available here: this command isn't part of a cluster job";
pub const NOT_LOADED: &str = "secret isn't available yet: this machine's cluster is still loading";
const MAX_NAMES: usize = 16;
const TEXT_MAX: usize = 4096;

#[derive(Clone, Debug)]
pub enum OnDisk {
    Nothing,
    Unreadable,
    Cluster { vault: NodeId, me: Option<NodeId> },
}

pub type Disk = Arc<dyn Fn() -> OnDisk + Send + Sync>;

type Strings = BTreeMap<String, String>;

pub trait Asker: Send + Sync {
    fn request(&self, vault: &NodeId, args: Value) -> Result<Value, ApiError>;
}

pub trait UseAudit: Send + Sync {
    fn used(&self, vault: &NodeId, job: &str, ok: bool);
}

pub struct GrantUse {
    store: Arc<GrantStore>,
    parts: (Arc<TailMasks>, Arc<GrantMarks>, Arc<JobRoots>),
    view: Arc<dyn View>,
    asks: (Arc<dyn Asker>, Arc<dyn UseAudit>),
    procs: Arc<dyn Processes>,
    clock: Clock,
    disk: Disk,
}

fn refused(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, message)
}

fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

pub fn keys_of(state: &ClusterState, job: &str) -> Vec<String> {
    let Some(found) = state.jobs.get(job) else {
        return Vec::new();
    };
    let key = if found.spec.tool == Tool::Shell {
        Some(format!("{SESSION_PREFIX}{job}"))
    } else {
        found.session.clone()
    };
    key.into_iter().filter(|k| valid_key(k)).collect()
}

pub fn keys_here(state: Option<&ClusterState>, roots: &JobRoots, job: &str) -> Vec<String> {
    let mut keys = state.map(|s| keys_of(s, job)).unwrap_or_default();
    for key in roots.keys(job) {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

fn is_live(state: JobState) -> bool {
    matches!(state, JobState::Running | JobState::Dispatching)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Standing {
    Live,
    Pending,
}

fn canonical(list: &[Value]) -> Result<Vec<String>, ApiError> {
    let bad = || ApiError::bad_request("vault.use takes {names: [name, …], folder, command}");
    if list.is_empty() || list.len() > MAX_NAMES {
        return Err(bad());
    }
    let mut names: Vec<String> = Vec::new();
    for item in list {
        let name = item.as_str().ok_or_else(bad)?.to_lowercase();
        if !valid_name(&name) {
            return Err(failed("No secret by that name."));
        }
        if !names.contains(&name) {
            names.push(name);
        }
    }
    Ok(names)
}

fn text_of(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(Value::as_str)
        .map(|t| t.chars().take(TEXT_MAX).collect())
        .unwrap_or_default()
}

impl GrantUse {
    pub fn new(
        store: Arc<GrantStore>,
        parts: (Arc<TailMasks>, Arc<GrantMarks>, Arc<JobRoots>),
        view: Arc<dyn View>,
        asks: (Arc<dyn Asker>, Arc<dyn UseAudit>),
        (procs, clock): (Arc<dyn Processes>, Clock),
    ) -> GrantUse {
        GrantUse {
            store,
            parts,
            view,
            asks,
            procs,
            clock,
            disk: Arc::new(|| OnDisk::Nothing),
        }
    }

    pub fn with_disk(mut self, disk: Disk) -> GrantUse {
        self.disk = disk;
        self
    }

    fn known(&self, origin: &Origin) -> Result<(ClusterState, NodeId), ApiError> {
        if origin.tier != OriginTier::Node {
            return Err(refused("only another node does that"));
        }
        let state = self
            .view
            .state()
            .ok_or_else(|| failed("this node is in no cluster"))?;
        let me = self
            .view
            .me()
            .ok_or_else(|| failed("this node is in no cluster"))?;
        Ok((state, me))
    }

    fn standing(&self, state: &ClusterState, me: &NodeId, job: &str) -> Option<Standing> {
        match state.jobs.get(job) {
            Some(found) if found.spec.node == *me && is_live(found.state) => Some(Standing::Live),
            Some(_) => None,
            None if self.parts.2.is_live(job) => Some(Standing::Live),
            None => Some(Standing::Pending),
        }
    }

    fn live_here(&self, state: &ClusterState, me: &NodeId, job: &str) -> bool {
        self.standing(state, me, job) == Some(Standing::Live)
    }

    fn not_here(state: &ClusterState, me: &NodeId, job: &str) -> ApiError {
        match state.jobs.get(job) {
            Some(found) if found.spec.node == *me => failed(format!("job {job} isn't running")),
            _ => failed(format!("job {job} isn't running on this machine")),
        }
    }

    pub fn arrived(&self) -> u64 {
        self.parts.2.now_seq()
    }

    pub fn deliver(&self, origin: &Origin, args: &Value) -> Result<Value, ApiError> {
        self.deliver_at(origin, args, self.parts.2.now_seq())
    }

    pub fn deliver_at(&self, origin: &Origin, args: &Value, since: u64) -> Result<Value, ApiError> {
        let (state, _) = self.known(origin)?;
        if origin.node != state.vault || state.home != state.vault {
            return Err(refused(
                "only the vault node, while it is the primary, delivers a secret",
            ));
        }
        let usage = "grant.deliver takes {job, name, value, expiresMs}";
        let job = args
            .get("job")
            .and_then(Value::as_str)
            .filter(|j| valid_job(j))
            .ok_or_else(|| ApiError::bad_request(usage))?;
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_lowercase)
            .filter(|n| valid_name(n))
            .ok_or_else(|| ApiError::bad_request(usage))?;
        let raw = args
            .get("value")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request(usage))?;
        let asked = args
            .get("expiresMs")
            .and_then(Value::as_i64)
            .ok_or_else(|| ApiError::bad_request(usage))?;
        let value = GrantValue::new(raw.to_string());
        let env = decode_env(&value).map_err(failed)?;
        let masked: Vec<(String, GrantValue)> = env
            .into_values()
            .filter(|v| !v.expose().is_empty())
            .map(|v| (name.clone(), v))
            .collect();
        TailMasks::check(&masked).map_err(failed)?;
        if self.parts.2.is_ended(job) {
            return Err(failed(format!("job {job} isn't running")));
        }
        let now = (self.clock)();
        let expires = asked.min(now.saturating_add(GRANT_TTL_MS));
        if expires <= now {
            return Err(failed("the grant has already expired"));
        }
        let _gate = self.parts.2.gate();
        if self.parts.2.is_ended(job) || self.parts.2.revoked_since(job, &name, since) {
            return Err(failed(format!("job {job} isn't running")));
        }
        let (state, me) = match (self.view.state(), self.view.me()) {
            (Some(state), Some(me)) => (state, me),
            _ => return Err(failed(format!("job {job} isn't running"))),
        };
        let standing = self
            .standing(&state, &me, job)
            .ok_or_else(|| Self::not_here(&state, &me, job))?;
        let keys = match standing {
            Standing::Live => keys_here(Some(&state), &self.parts.2, job),
            Standing::Pending => Vec::new(),
        };
        if !masked.is_empty() {
            self.parts.1.mark_with(job, &keys).map_err(failed)?;
            self.parts.0.add(job, masked).map_err(failed)?;
        }
        self.store
            .put(&GrantFile {
                job: job.to_string(),
                name: name.clone(),
                expires_ms: expires,
                value,
            })
            .map_err(|why| failed(format!("the grant for {name} can't be stored: {why}")))?;
        if standing == Standing::Pending {
            self.parts.2.hold(job, now);
        }
        Ok(json!({"stored": name}))
    }

    pub fn starting(&self, origin: &Origin, args: &Value, tool: StartTool) -> Option<String> {
        if origin.tier != OriginTier::App {
            return None;
        }
        let state = self.view.state()?;
        if origin.node != state.home {
            return None;
        }
        let job = args
            .get("job")
            .and_then(Value::as_str)
            .filter(|j| valid_job(j))?;
        self.parts.2.begin(job, tool).then(|| job.to_string())
    }

    pub fn started(&self, job: &str, result: &Result<Value, ApiError>) {
        match result {
            Ok(reply) => {
                let session = ["session", "id", "jobId"]
                    .iter()
                    .find_map(|field| reply.get(*field).and_then(Value::as_str));
                if let Some(key) = session {
                    self.parts.2.note_session(job, key);
                }
            }
            Err(_) => {
                let _gate = self.parts.2.gate();
                self.parts.2.end(job);
                let _ = self.store.remove_job(job);
                self.parts.0.end_job(job);
            }
        }
    }

    pub fn revoke(&self, origin: &Origin, args: &Value) -> Result<Value, ApiError> {
        let (state, _) = self.known(origin)?;
        if origin.node != state.home && origin.node != state.vault {
            return Err(refused("only the home or the vault node revokes a secret"));
        }
        let usage = "grant.revoke takes {job, names?}";
        let job = args
            .get("job")
            .and_then(Value::as_str)
            .filter(|j| valid_job(j))
            .ok_or_else(|| ApiError::bad_request(usage))?;
        let names: Option<Vec<String>> = match args.get("names") {
            None | Some(Value::Null) => None,
            Some(Value::Array(list)) => Some(
                list.iter()
                    .map(|n| n.as_str().map(str::to_string))
                    .collect::<Option<_>>()
                    .ok_or_else(|| ApiError::bad_request(usage))?,
            ),
            Some(_) => return Err(ApiError::bad_request(usage)),
        };
        let _gate = self.parts.2.gate();
        self.parts.2.note_revoke(job, names.as_deref());
        let result = match &names {
            None => {
                self.parts.2.release_pending(job);
                self.store.remove_job(job)
            }
            Some(names) => {
                let mut first = Ok(());
                for name in names {
                    let each = self.store.remove(job, name);
                    if first.is_ok() {
                        first = each;
                    }
                }
                first
            }
        };
        result.map_err(|why| failed(format!("the grants of {job} can't all be removed: {why}")))?;
        Ok(json!({}))
    }

    fn held(&self, job: &str, names: &[String]) -> Result<Vec<Option<GrantFile>>, ApiError> {
        let now = (self.clock)();
        names
            .iter()
            .map(|name| {
                self.store
                    .get(job, name, now)
                    .map_err(|why| failed(format!("the grant for {name} can't be read: {why}")))
            })
            .collect()
    }

    fn fetch(
        &self,
        (job, vault): (&str, &NodeId),
        names: &[String],
        (folder, command): (String, String),
    ) -> Result<Vec<GrantFile>, ApiError> {
        let mut held = self.held(job, names)?;
        let missing: Vec<&String> = names
            .iter()
            .zip(&held)
            .filter(|(_, g)| g.is_none())
            .map(|(n, _)| n)
            .collect();
        if missing.is_empty() {
            return Ok(held.into_iter().flatten().collect());
        }
        let args = json!({"job": job, "names": missing, "command": command, "folder": folder});
        let reply = self.asks.0.request(vault, args)?;
        if let Some(why) = reply.get("waiting").and_then(Value::as_str) {
            return Err(failed(format!(
                "secret isn't available yet: waiting for {why}"
            )));
        }
        held = self.held(job, names)?;
        match names.iter().zip(&held).find(|(_, g)| g.is_none()) {
            Some((name, _)) => Err(failed(format!("{name} wasn't delivered"))),
            None => Ok(held.into_iter().flatten().collect()),
        }
    }

    fn use_for(
        &self,
        caller: &Caller,
        args: &Value,
        (state, me): (&ClusterState, &NodeId),
        seen: &mut String,
    ) -> Result<Value, ApiError> {
        let bad = || ApiError::bad_request("vault.use takes {names: [name, …], folder, command}");
        let names = canonical(
            args.get("names")
                .and_then(Value::as_array)
                .ok_or_else(bad)?,
        )?;
        let pid = caller.pid.ok_or_else(|| refused(NOT_A_JOB))?;
        let job = self
            .parts
            .2
            .job_of(self.procs.as_ref(), pid)
            .ok_or_else(|| refused(NOT_A_JOB))?;
        *seen = job.clone();
        if !self.live_here(state, me, &job) {
            return Err(refused(format!("job {job} isn't running")));
        }
        let said = (text_of(args, "folder"), text_of(args, "command"));
        let grants = self.fetch((&job, &state.vault), &names, said)?;
        let (env, hide) = environment(&grants)?;
        Ok(reply(&env, &hide))
    }
}

impl WorkerUse for GrantUse {
    fn use_secrets(&self, caller: &Caller, args: &Value) -> Option<Result<Value, ApiError>> {
        let (Some(state), Some(me)) = (self.view.state(), self.view.me()) else {
            return match (self.disk)() {
                OnDisk::Nothing => None,
                OnDisk::Cluster { vault, me } => {
                    let own = self.view.me().or(me);
                    own.is_none_or(|own| own != vault)
                        .then(|| Err(failed(NOT_LOADED)))
                }
                OnDisk::Unreadable => Some(Err(failed(NOT_LOADED))),
            };
        };
        if state.vault == me {
            return None;
        }
        let mut job = String::new();
        let result = self.use_for(caller, args, (&state, &me), &mut job);
        self.asks.1.used(&state.vault, &job, result.is_ok());
        Some(result)
    }
}

pub struct UseCalls {
    inner: Arc<dyn LocalCalls>,
    uses: Arc<GrantUse>,
}

impl UseCalls {
    pub fn new(inner: Arc<dyn LocalCalls>, uses: Arc<GrantUse>) -> UseCalls {
        UseCalls { inner, uses }
    }
}

pub fn start_tool(op: &str) -> Option<StartTool> {
    match op {
        ops::SESSION_NEW => Some(StartTool::Claude),
        ops::AGY_SEND => Some(StartTool::Agy),
        _ => None,
    }
}

impl LocalCalls for UseCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        let arrived = self.uses.arrived();
        match op {
            GRANT_DELIVER => self.uses.deliver_at(origin, args, arrived),
            GRANT_REVOKE => self.uses.revoke(origin, args),
            _ => match start_tool(op) {
                Some(tool) => {
                    let job = self.uses.starting(origin, args, tool);
                    let result = self.inner.call(op, args, origin);
                    if let Some(job) = job {
                        self.uses.started(&job, &result);
                    }
                    result
                }
                None => self.inner.call(op, args, origin),
            },
        }
    }
}

pub fn environment(grants: &[GrantFile]) -> Result<(Strings, Strings), ApiError> {
    let mut env = BTreeMap::new();
    let mut hide = BTreeMap::new();
    for grant in grants {
        let vars = decode_env(&grant.value).map_err(failed)?;
        let mask = format!("[secret:{}]", grant.name);
        for (var, value) in vars {
            if value.expose().is_empty() {
                continue;
            }
            hide.extend(hide_forms(value.expose(), &mask));
            env.insert(var, value.expose().to_string());
        }
    }
    Ok((env, hide))
}

pub fn reply(env: &Strings, hide: &Strings) -> Value {
    let mut out = Map::new();
    out.insert("env".into(), json!(env));
    out.insert("hide".into(), json!(hide));
    Value::Object(out)
}

#[cfg(test)]
#[path = "grant_use_tests.rs"]
pub(crate) mod tests;
