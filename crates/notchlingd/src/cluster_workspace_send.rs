use crate::cluster_blobs::{
    create_private, is_real_file, private_dir, remove_plain, NODE_PUT, NODE_PUT_END, OUTGOING_DIR,
};
use crate::cluster_node::{ClusterNode, LOCAL_WAIT};
use crate::cluster_workspace::{bad, failed, run_calls, NODE_PREPARE, NODE_RETURN};
use crate::op_group::OpGroup;
use notchling_engine::bridge::net::crypto::{base64, base64_decode};
use notchling_engine::cluster::blob::{chunks, safe_name, Digest, Incoming, MAX_BLOB};
use notchling_engine::cluster::core::{LocalCmd, WORKSPACE_CALL_MS};
use notchling_engine::cluster::jobs::job_id;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::workspace::{
    back_branch, facts_calls, parse_facts, plan_fetch_back, plan_send, FactsOut, GitCall,
    RepoFacts, NO_REMOTE,
};
use notchling_engine::cluster::NodeId;
use notchling_engine::control::Launcher;
use notchling_local_api::{ops, ApiError, Caller, OpSpec};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub const SIDECAR_DIR: &str = "workspaces";
const SIDECAR_MAX: u64 = 8 * 1024;

pub const WORKSPACE_WAIT: Duration = Duration::from_millis(WORKSPACE_CALL_MS + 5_000);

pub trait Peers: Send + Sync {
    fn call(&self, node: &NodeId, op: &str, args: Value) -> Result<Value, ApiError>;
}

pub struct NodePeers {
    node: Arc<OnceLock<Arc<ClusterNode>>>,
}

impl NodePeers {
    pub fn new(node: Arc<OnceLock<Arc<ClusterNode>>>) -> NodePeers {
        NodePeers { node }
    }
}

impl Peers for NodePeers {
    fn call(&self, node: &NodeId, op: &str, args: Value) -> Result<Value, ApiError> {
        let me = self
            .node
            .get()
            .ok_or_else(|| failed("this machine can't be a cluster node"))?;
        let origin = Origin {
            node: me.me().clone(),
            tier: OriginTier::App,
        };
        let within = if op.starts_with("workspace.") {
            WORKSPACE_WAIT
        } else {
            LOCAL_WAIT
        };
        me.local_within(
            LocalCmd::Forward {
                node: node.clone(),
                op: op.to_string(),
                args,
                origin,
            },
            within,
        )
    }
}

pub trait CodeSender: Send + Sync {
    fn send(&self, node: &NodeId, name: &str, key: &str, cwd: &str) -> Result<String, ApiError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    Folder,
    Send,
}

pub fn code_mode(args: &Value) -> Result<Code, ApiError> {
    match args.get("code") {
        None | Some(Value::Null) => Ok(Code::Folder),
        Some(Value::String(s)) if s == "folder" => Ok(Code::Folder),
        Some(Value::String(s)) if s == "send" => Ok(Code::Send),
        Some(_) => Err(bad("code is send or folder")),
    }
}

pub struct Sender {
    cluster: PathBuf,
    git: Arc<dyn Launcher>,
    peers: Arc<dyn Peers>,
}

fn text_of(path: &Path) -> Result<&str, ApiError> {
    path.to_str()
        .ok_or_else(|| failed("a folder name isn't plain text"))
}

impl Sender {
    pub fn new(cluster: PathBuf, git: Arc<dyn Launcher>, peers: Arc<dyn Peers>) -> Arc<Sender> {
        Arc::new(Sender {
            cluster,
            git,
            peers,
        })
    }

    fn one(&self, call: &GitCall) -> Result<Option<String>, ApiError> {
        let outputs = run_calls(
            self.git.as_ref(),
            &[GitCall {
                allow_failure: true,
                ..call.clone()
            }],
        )
        .map_err(failed)?;
        Ok(outputs
            .into_iter()
            .next()
            .filter(|out| out.code == 0)
            .map(|out| out.output))
    }

    pub fn read_facts(&self, cwd: &str) -> Result<RepoFacts, ApiError> {
        let calls = facts_calls(cwd).map_err(bad)?;
        let root = self
            .one(&calls.root)?
            .ok_or_else(|| bad("That folder isn't in a git repository"))?;
        let head = self
            .one(&calls.head)?
            .ok_or_else(|| bad("That repository has no commits yet"))?;
        let out = FactsOut {
            root,
            head,
            branch: self.one(&calls.branch)?,
            origin: self.one(&calls.origin)?,
            upstream: self.one(&calls.upstream)?,
        };
        parse_facts(out).map_err(bad)
    }

    fn push(&self, node: &NodeId, transfer: &str, file: &Path) -> Result<(), ApiError> {
        let mut open = std::fs::File::open(file).map_err(|e| failed(e.to_string()))?;
        let len = open.metadata().map_err(|e| failed(e.to_string()))?.len();
        if len > MAX_BLOB {
            return Err(bad("The commits to send are over 256 MB"));
        }
        let mut digest = Digest::default();
        for (offset, size) in chunks(len) {
            let mut data = vec![0; size];
            open.read_exact(&mut data)
                .map_err(|e| failed(e.to_string()))?;
            digest.update(&data);
            let args = json!({"transfer": transfer, "offset": offset, "data": base64(&data)});
            let reply = self.peers.call(node, NODE_PUT, args)?;
            if reply.get("next").and_then(Value::as_u64) != Some(offset + size as u64) {
                return Err(failed("the other machine lost count of what it received"));
            }
        }
        let args = json!({"transfer": transfer, "sha256": digest.finish()});
        self.peers.call(node, NODE_PUT_END, args)?;
        Ok(())
    }

    fn sidecar(&self, job: &str) -> Result<PathBuf, ApiError> {
        let dir = private_dir(&self.cluster, SIDECAR_DIR)
            .map_err(|e| failed(format!("the workspaces folder: {e}")))?;
        Ok(dir.join(format!("{job}.json")))
    }

    pub fn send_code(
        &self,
        node: &NodeId,
        name: &str,
        key: &str,
        cwd: &str,
    ) -> Result<String, ApiError> {
        let facts = self.read_facts(cwd)?;
        if facts.origin.is_none() {
            return Err(bad(NO_REMOTE.replace("the Worker", name)));
        }
        let job = job_id(key);
        let real =
            std::fs::canonicalize(cwd).map_err(|e| bad(format!("no folder at {cwd}: {e}")))?;
        let rel = real
            .strip_prefix(&facts.root)
            .map_err(|_| bad("The folder isn't inside its repository"))?
            .to_path_buf();
        let out_dir = private_dir(&self.cluster, OUTGOING_DIR)
            .map_err(|e| failed(format!("the outgoing folder: {e}")))?;
        let file = out_dir.join(format!("{job}.send.bundle"));
        let plan = plan_send(&facts, text_of(&file)?).map_err(bad)?;
        let mut transfer = None;
        if let Some(call) = &plan.bundle {
            remove_plain(&file);
            let made = run_calls(self.git.as_ref(), std::slice::from_ref(call)).map_err(failed);
            let sent = made.and_then(|_| {
                let id = format!("{job}-code");
                self.push(node, &id, &file).map(|()| id)
            });
            remove_plain(&file);
            transfer = Some(sent?);
        }
        let reply = self.peers.call(
            node,
            NODE_PREPARE,
            json!({"job": job, "facts": facts, "transfer": transfer}),
        )?;
        let tree = reply
            .get("cwd")
            .and_then(Value::as_str)
            .ok_or_else(|| failed("the other machine didn't say where the code is"))?;
        let note = json!({
            "node": node,
            "name": name,
            "root": facts.root,
            "branch": facts.branch,
            "head": facts.head,
        });
        let path = self.sidecar(&job)?;
        remove_plain(&path);
        let mut out = create_private(&path).map_err(|e| failed(format!("the job's note: {e}")))?;
        out.write_all(note.to_string().as_bytes())
            .map_err(|e| failed(e.to_string()))?;
        if rel.as_os_str().is_empty() {
            return Ok(tree.to_string());
        }
        Ok(Path::new(tree).join(rel).to_string_lossy().into_owned())
    }

    fn note(&self, job: &str) -> Result<(NodeId, String, String), ApiError> {
        let path = self.sidecar(job)?;
        if !is_real_file(&path) {
            return Err(bad("No code was sent to a machine for that job"));
        }
        let file = std::fs::File::open(&path).map_err(|e| failed(e.to_string()))?;
        let mut text = String::new();
        file.take(SIDECAR_MAX)
            .read_to_string(&mut text)
            .map_err(|e| failed(e.to_string()))?;
        let note: Value =
            serde_json::from_str(&text).map_err(|_| failed("the job's note is damaged"))?;
        let field = |name: &str| note.get(name).and_then(Value::as_str).map(str::to_string);
        let node = field("node")
            .and_then(|n| NodeId::parse(&n))
            .ok_or_else(|| failed("the job's note is damaged"))?;
        let (Some(name), Some(root)) = (field("name"), field("root")) else {
            return Err(failed("the job's note is damaged"));
        };
        Ok((node, name, root))
    }

    pub fn bring_back(&self, job: &str) -> Result<Value, ApiError> {
        if !safe_name(job) {
            return Err(bad("job is a short name"));
        }
        let (node, name, root) = self.note(job)?;
        let branch = back_branch(&name, job).map_err(bad)?;
        let out_dir = private_dir(&self.cluster, OUTGOING_DIR)
            .map_err(|e| failed(format!("the outgoing folder: {e}")))?;
        let file = out_dir.join(format!("{job}.back.bundle"));
        remove_plain(&file);
        let pulled = self.pull(&node, job, &file);
        let result = pulled.and_then(|()| {
            let calls = plan_fetch_back(&root, text_of(&file)?, &name, job).map_err(bad)?;
            run_calls(self.git.as_ref(), &calls).map_err(failed)?;
            Ok(json!({"job": job, "branch": branch, "node": name}))
        });
        remove_plain(&file);
        result
    }

    fn pull(&self, node: &NodeId, job: &str, file: &Path) -> Result<(), ApiError> {
        let mut out = create_private(file).map_err(|e| failed(format!("the bundle file: {e}")))?;
        let mut incoming = Incoming::default();
        let mut from = 0u64;
        let limit = MAX_BLOB / notchling_engine::cluster::blob::CHUNK as u64 + 4;
        for _ in 0..limit {
            let reply = self
                .peers
                .call(node, NODE_RETURN, json!({"job": job, "from": from}))?;
            let data = reply
                .get("data")
                .and_then(Value::as_str)
                .and_then(base64_decode)
                .ok_or_else(|| failed("the other machine sent damaged work"))?;
            incoming.put(from, &data).map_err(failed)?;
            out.write_all(&data).map_err(|e| failed(e.to_string()))?;
            from += data.len() as u64;
            if reply.get("done") == Some(&json!(true)) {
                let sha = reply
                    .get("sha256")
                    .and_then(Value::as_str)
                    .ok_or_else(|| failed("the other machine sent no hash"))?;
                return incoming.finish(sha).map_err(failed);
            }
            if data.is_empty() {
                return Err(failed("the other machine stopped sending"));
            }
        }
        Err(failed("the work to bring back is too big"))
    }
}

impl CodeSender for Sender {
    fn send(&self, node: &NodeId, name: &str, key: &str, cwd: &str) -> Result<String, ApiError> {
        self.send_code(node, name, key, cwd)
    }
}

pub struct ReturnOps {
    sender: Arc<Sender>,
}

impl ReturnOps {
    pub fn new(sender: Arc<Sender>) -> Arc<ReturnOps> {
        Arc::new(ReturnOps { sender })
    }
}

impl OpGroup for ReturnOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::CLUSTER_RETURN).then_some(OpSpec::APP)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if op != ops::CLUSTER_RETURN {
            return Err(ApiError::internal(format!("{op} has no handler")));
        }
        let job = args
            .get("job")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("cluster.return takes {\"job\": \"j-...\"}"))?;
        self.sender.bring_back(job)
    }
}

#[cfg(test)]
#[path = "cluster_workspace_send_tests.rs"]
mod tests;
