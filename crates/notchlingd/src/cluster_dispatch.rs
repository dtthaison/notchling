use crate::acting_ops::bad;
use crate::cluster_dispatch_delegate::{args_of, check, cut, DelegateRunner};
use crate::cluster_node::{ClusterNode, LocalCalls, DELEGATE_WAIT, LOCAL_WAIT};
use crate::cluster_workspace_send::{code_mode, Code, CodeSender};
use crate::op_group::OpGroup;
use crate::shell_jobs::{Reporter, ShellJobs};
use notchling_engine::cluster::core::{ATTACH_CALL_MS, CALL_MS};
use notchling_engine::cluster::shell_job::{CODEX_SECRETS, MAX_COMMAND};
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{JobSpec, JobState, LocalCmd, NodeId, Qualified, Tool};
use notchling_engine::vault::valid_name;
use notchling_local_api::{ops, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

pub const NODE_SHELL_RUN: &str = "shell.run";
pub const NODE_DELEGATE_RUN: &str = "delegate.run";
pub const NODE_OPS: [&str; 2] = [NODE_SHELL_RUN, NODE_DELEGATE_RUN];
pub const JOBS_TOPIC: &str = "jobs";
pub const JOB_STATE: &str = "job.state";

const KEY_MAX: usize = 128;
const CWD_MAX: usize = 4096;
const TEXT_MAX: usize = 64 * 1024;

fn opt_text<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, ApiError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(bad(format!("{key} is text"))),
    }
}

fn nothing_else(args: &Value, keys: &[&str], tool: &str) -> Result<(), ApiError> {
    match keys
        .iter()
        .find(|k| !matches!(args.get(**k), None | Some(Value::Null)))
    {
        Some(key) => Err(bad(format!("{key} isn't taken by {tool}"))),
        None => Ok(()),
    }
}

fn nonblank<'a>(value: Option<&'a str>, what: &str, max: usize) -> Result<&'a str, ApiError> {
    let text = value
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| bad(format!("{what} is needed")))?;
    if text.len() > max || text.contains('\0') {
        return Err(bad(format!("{what} is too long or has a NUL byte")));
    }
    Ok(text)
}

fn plain_spec(node: NodeId, args: &Value) -> Result<JobSpec, ApiError> {
    let tool = match opt_text(args, "tool")? {
        Some("claude") => Tool::Claude,
        Some("agy") => Tool::Agy,
        Some("shell") => Tool::Shell,
        Some("codex") => Tool::Codex,
        _ => return Err(bad("tool is claude, codex, agy or shell")),
    };
    let cwd = opt_text(args, "cwd")?
        .filter(|c| c.starts_with('/') && c.len() <= CWD_MAX && !c.contains('\0'))
        .ok_or_else(|| bad("cwd is an absolute path"))?;
    let (model, text, command) = (
        opt_text(args, "model")?,
        opt_text(args, "text")?,
        opt_text(args, "command")?,
    );
    let fields = match tool {
        Tool::Claude => {
            nothing_else(args, &["text", "command"], "a Claude Code session")?;
            match model.filter(|m| !m.trim().is_empty()) {
                Some(m) => json!({"flags": ["--model", m]}),
                None => json!({}),
            }
        }
        Tool::Agy => {
            nothing_else(args, &["model", "command"], "an Antigravity session")?;
            json!({"text": nonblank(text, "text", TEXT_MAX)?})
        }
        Tool::Shell => {
            nothing_else(args, &["model", "text"], "a shell job")?;
            json!({"command": nonblank(command, "command", MAX_COMMAND)?})
        }
        Tool::Codex => {
            nothing_else(args, &["command"], "a Codex session")?;
            let mut fields = serde_json::Map::new();
            fields.insert("text".into(), json!(nonblank(text, "text", TEXT_MAX)?));
            for key in ["model", "effort"] {
                if let Some(word) = opt_text(args, key)?.filter(|w| !w.trim().is_empty()) {
                    fields.insert(key.into(), json!(word));
                }
            }
            Value::Object(fields)
        }
    };
    Ok(JobSpec {
        node,
        tool,
        cwd: cwd.to_string(),
        args: fields,
        secrets: Vec::new(),
    })
}

fn delegate_spec(node: NodeId, args: &Value) -> Result<JobSpec, ApiError> {
    let list = args_of(&args["delegate"])?;
    let parsed = check(&list)?;
    Ok(JobSpec {
        node,
        tool: Tool::Agy,
        cwd: parsed.options.folder,
        args: json!({"delegate": list}),
        secrets: Vec::new(),
    })
}

pub const MAX_SECRETS: usize = 16;

fn secret_names(args: &Value) -> Result<Vec<String>, ApiError> {
    let list = match args.get("secrets") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(list)) => list,
        Some(_) => return Err(bad("secrets is a list of names")),
    };
    if list.len() > MAX_SECRETS {
        return Err(bad(format!(
            "at most {MAX_SECRETS} secrets go with one job"
        )));
    }
    let mut names: Vec<String> = Vec::new();
    for item in list {
        let name = item
            .as_str()
            .filter(|n| valid_name(n))
            .ok_or_else(|| bad("secrets takes names: letters, digits, - _ or ."))?
            .to_lowercase();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    Ok(names)
}

pub fn build(args: &Value, tier: OriginTier) -> Result<(String, JobSpec), ApiError> {
    let usage = "cluster.dispatch takes {\"node\": id, \"key\": text, \"tool\": claude|codex|agy|shell, \"cwd\": path, ...} or {\"node\", \"key\", \"delegate\": [args]}";
    let key = opt_text(args, "key")?
        .filter(|k| !k.trim().is_empty() && k.len() <= KEY_MAX)
        .ok_or_else(|| bad(usage))?;
    let node = opt_text(args, "node")?
        .and_then(NodeId::parse)
        .ok_or_else(|| bad(usage))?;
    let secrets = secret_names(args)?;
    let mut spec = if args.get("delegate").is_some() {
        delegate_spec(node, args)?
    } else {
        plain_spec(node, args)?
    };
    if !secrets.is_empty() && spec.is_delegate() {
        return Err(bad("a delegate job takes no secrets"));
    }
    if !secrets.is_empty() && spec.tool == Tool::Codex {
        return Err(bad(CODEX_SECRETS));
    }
    spec.secrets = secrets;
    if tier != OriginTier::App && !spec.is_delegate() {
        return Err(ApiError::new(
            ErrorCode::Forbidden,
            "only the Notchling app starts that on another node",
        ));
    }
    Ok((key.to_string(), spec))
}

#[derive(Default)]
pub struct NodeReporter {
    node: OnceLock<Weak<ClusterNode>>,
}

impl NodeReporter {
    pub fn attach(&self, node: Arc<ClusterNode>) {
        let _ = self.node.set(Arc::downgrade(&node));
    }
}

impl Reporter for NodeReporter {
    fn report(&self, job: &str, state: JobState, detail: Option<String>) {
        let Some(node) = self.node.get().and_then(Weak::upgrade) else {
            return;
        };
        let data = json!({"job": job, "state": state, "detail": detail});
        node.tell(JOBS_TOPIC, JOB_STATE, data.clone());
        node.submit().send(LocalCmd::JobEvent {
            name: JOB_STATE.to_string(),
            data,
        });
    }
}

impl crate::codex_ops::JobLink for NodeReporter {
    fn session_started(&self, job: &str, session: &str) {
        let Some(node) = self.node.get().and_then(Weak::upgrade) else {
            return;
        };
        let data = json!({"job": job, "state": JobState::Running, "session": session});
        node.tell(JOBS_TOPIC, JOB_STATE, data.clone());
        node.submit().send(LocalCmd::JobEvent {
            name: JOB_STATE.to_string(),
            data,
        });
    }

    fn start_failed(&self, job: &str, why: &str) {
        self.report(job, JobState::Failed, Some(why.to_string()));
    }
}

pub struct DispatchOps {
    node: Result<Arc<ClusterNode>, String>,
    shells: Arc<ShellJobs>,
    code: Option<Arc<dyn CodeSender>>,
}

fn name_of(status: &Value, node: &NodeId) -> String {
    status
        .get("members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|m| m.get("id") == Some(&json!(node)))
        .and_then(|m| m.get("name").and_then(Value::as_str))
        .unwrap_or(node.as_str())
        .to_string()
}

impl DispatchOps {
    pub fn new(node: Result<Arc<ClusterNode>, String>, shells: Arc<ShellJobs>) -> Arc<DispatchOps> {
        DispatchOps::with_code(node, shells, None)
    }

    pub fn with_code(
        node: Result<Arc<ClusterNode>, String>,
        shells: Arc<ShellJobs>,
        code: Option<Arc<dyn CodeSender>>,
    ) -> Arc<DispatchOps> {
        Arc::new(DispatchOps { node, shells, code })
    }

    fn with_sent_code(&self, tier: OriginTier, args: &Value) -> Result<Value, ApiError> {
        if tier != OriginTier::App {
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "only the Notchling app sends code to another node",
            ));
        }
        if args.get("delegate").is_some() {
            return Err(bad("code isn't taken by a delegate job"));
        }
        let (key, spec) = build(args, tier)?;
        let node = self.node()?;
        if spec.node == *node.me() {
            return Ok(args.clone());
        }
        let sender = self
            .code
            .as_ref()
            .ok_or_else(|| bad("sending code isn't available here"))?;
        let name = name_of(&node.status(), &spec.node);
        let cwd = sender.send(&spec.node, &name, &key, &spec.cwd)?;
        let mut sent = args.clone();
        sent["cwd"] = json!(cwd);
        Ok(sent)
    }

    fn node(&self) -> Result<&Arc<ClusterNode>, ApiError> {
        self.node.as_ref().map_err(|why| {
            ApiError::new(
                ErrorCode::Failed,
                format!("this machine can't be a cluster node: {why}"),
            )
        })
    }

    fn dispatch(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let tier = if caller.tier == Tier::App {
            OriginTier::App
        } else {
            OriginTier::Agent
        };
        let sent;
        let args = if code_mode(args)? == Code::Send {
            sent = self.with_sent_code(tier, args)?;
            &sent
        } else {
            args
        };
        let (key, spec) = build(args, tier)?;
        let within = if spec.is_delegate() {
            DELEGATE_WAIT + LOCAL_WAIT
        } else if spec.secrets.is_empty() {
            LOCAL_WAIT + Duration::from_millis(CALL_MS)
        } else {
            LOCAL_WAIT + Duration::from_millis(CALL_MS + ATTACH_CALL_MS)
        };
        self.node()?
            .local_within(LocalCmd::Dispatch { key, spec, tier }, within)
    }

    fn stop(&self, args: &Value) -> Result<Value, ApiError> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("shell.stop takes {\"id\": \"shell:<job>\"}"))?;
        let bare = match Qualified::parse(id) {
            Some(q) if self.node.as_ref().is_ok_and(|n| *n.me() == q.node) => q.id,
            Some(_) => return Err(bad("that job is on another node")),
            None => id.to_string(),
        };
        self.shells.stop(&json!({"id": bare}))
    }
}

impl OpGroup for DispatchOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::CLUSTER_DISPATCH => Some(OpSpec::AGENT),
            ops::SHELL_STOP => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::CLUSTER_DISPATCH => self.dispatch(caller, args),
            ops::SHELL_STOP => self.stop(args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

pub type Takes = Arc<dyn Fn() -> bool + Send + Sync>;

pub struct DispatchCalls {
    inner: Arc<dyn LocalCalls>,
    shells: Arc<ShellJobs>,
    runner: Arc<dyn DelegateRunner>,
    takes: Takes,
}

impl DispatchCalls {
    pub fn new(
        inner: Arc<dyn LocalCalls>,
        shells: Arc<ShellJobs>,
        runner: Arc<dyn DelegateRunner>,
        takes: Takes,
    ) -> DispatchCalls {
        DispatchCalls {
            inner,
            shells,
            runner,
            takes,
        }
    }

    fn delegate(&self, args: &Value) -> Result<Value, ApiError> {
        if !(self.takes)() {
            return Err(ApiError::new(
                ErrorCode::Failed,
                "this machine doesn't take delegated jobs",
            ));
        }
        let list = args_of(args.get("args").unwrap_or(&Value::Null))?;
        check(&list)?;
        let (exit, out, err) = self.runner.run(&list);
        Ok(json!({
            "exit": exit,
            "out": cut(self.shells.mask_text(&out)),
            "err": cut(self.shells.mask_text(&err)),
        }))
    }
}

impl LocalCalls for DispatchCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        match op {
            NODE_SHELL_RUN => self.shells.run(args),
            ops::SHELL_STOP => self.shells.stop(args),
            NODE_DELEGATE_RUN => self.delegate(args),
            _ => self.inner.call(op, args, origin),
        }
    }
}

#[cfg(test)]
#[path = "cluster_dispatch_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cluster_dispatch_masked_tests.rs"]
mod masked_tests;
