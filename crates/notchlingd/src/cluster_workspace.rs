use crate::cluster_blobs::{
    is_real_file, private_dir, remove_plain, sweep_old, Blobs, NODE_PUT, NODE_PUT_END, OUTGOING_DIR,
};
use crate::cluster_node::LocalCalls;
use notchling_engine::bridge::net::crypto::{base64, hex, random_bytes};
use notchling_engine::cluster::blob::{safe_name, Digest, CHUNK};
use notchling_engine::cluster::wire::Origin;
use notchling_engine::cluster::workspace::{
    plan_cleanup, plan_prepare, plan_return, repo_dir, scrubbed, worktree_dir, GitCall, RepoFacts,
    ENV,
};
use notchling_engine::control::{Command, LaunchOutput, Launcher};
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;
use std::time::SystemTime;

pub const NODE_PREPARE: &str = "workspace.prepare";
pub const NODE_RETURN: &str = "workspace.return";
pub const NODE_OPS: [&str; 4] = [NODE_PUT, NODE_PUT_END, NODE_PREPARE, NODE_RETURN];
pub const WORK_DIR: &str = "work";
const RETURN_SUFFIX: &str = ".return.bundle";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn bad(why: impl Into<String>) -> ApiError {
    ApiError::bad_request(why)
}

pub(crate) fn failed(why: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, why)
}

pub fn command_for(call: &GitCall, inherited: impl IntoIterator<Item = String>) -> Command {
    command_with(&call.argv(), inherited)
}

pub fn command_with(argv: &[String], inherited: impl IntoIterator<Item = String>) -> Command {
    let mut command = Command::from_args("git", argv);
    for name in scrubbed(inherited) {
        command = command.with_env(&name, None);
    }
    for (name, value) in ENV {
        command = command.with_env(name, Some(value));
    }
    command
}

fn first_line(out: &LaunchOutput) -> String {
    let text = if out.stderr.trim().is_empty() {
        &out.output
    } else {
        &out.stderr
    };
    text.lines().next().unwrap_or("").trim().to_string()
}

fn refusal(call: &GitCall, out: &LaunchOutput) -> String {
    let all = format!("{}{}", out.stderr, out.output);
    for (when, text) in &call.refusals {
        if when.is_empty() {
            return format!("{text}: {}", first_line(out));
        }
        if all.contains(when.as_str()) {
            return text.clone();
        }
    }
    format!("git exited {}: {}", out.code, first_line(out))
}

pub fn run_calls(git: &dyn Launcher, calls: &[GitCall]) -> Result<Vec<LaunchOutput>, String> {
    let mut outputs = Vec::new();
    for call in calls {
        if call
            .skip_if_exists
            .as_deref()
            .is_some_and(|path| std::fs::symlink_metadata(path).is_ok())
        {
            continue;
        }
        let command = command_for(
            call,
            std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()),
        );
        let out = git
            .run(&command, Duration::from_secs(call.timeout_secs))
            .map_err(|e| format!("git {e}"))?;
        if out.code != 0 && !call.allow_failure {
            return Err(refusal(call, &out));
        }
        outputs.push(out);
    }
    Ok(outputs)
}

fn text_of(path: &Path) -> Result<&str, ApiError> {
    path.to_str()
        .ok_or_else(|| failed("a folder name isn't plain text"))
}

#[derive(Default)]
pub struct Turns {
    held: Mutex<BTreeSet<String>>,
    wake: Condvar,
}

pub struct Turn<'a> {
    turns: &'a Turns,
    key: String,
}

impl Turns {
    pub fn claim(&self, key: &str) -> Turn<'_> {
        let mut held = lock(&self.held);
        while held.contains(key) {
            held = self.wake.wait(held).unwrap_or_else(|e| e.into_inner());
        }
        held.insert(key.to_string());
        Turn {
            turns: self,
            key: key.to_string(),
        }
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        lock(&self.turns.held).remove(&self.key);
        self.turns.wake.notify_all();
    }
}

pub struct Workspace {
    cluster: PathBuf,
    git: Arc<dyn Launcher>,
    blobs: Arc<Blobs>,
    turns: Turns,
}

impl Workspace {
    pub fn new(cluster: PathBuf, git: Arc<dyn Launcher>, blobs: Arc<Blobs>) -> Arc<Workspace> {
        Arc::new(Workspace {
            cluster,
            git,
            blobs,
            turns: Turns::default(),
        })
    }

    fn work_root(&self) -> Result<PathBuf, ApiError> {
        private_dir(&self.cluster, WORK_DIR).map_err(|e| failed(format!("the work folder: {e}")))
    }

    pub fn prepare(&self, args: &Value) -> Result<Value, ApiError> {
        let job = args
            .get("job")
            .and_then(Value::as_str)
            .filter(|j| safe_name(j))
            .ok_or_else(|| bad("job is a short name"))?;
        let facts: RepoFacts =
            serde_json::from_value(args.get("facts").cloned().unwrap_or(Value::Null))
                .map_err(|_| bad("facts describe the repository"))?;
        let transfer = match args.get("transfer") {
            None | Some(Value::Null) => None,
            Some(Value::String(t)) => Some(t.as_str()),
            Some(_) => return Err(bad("transfer is text")),
        };
        let root = self.work_root()?;
        let root = text_of(&root)?;
        let bundle = match transfer {
            Some(t) => Some(self.blobs.finished(t)?),
            None => None,
        };
        let bundle_text = match &bundle {
            Some(path) => Some(text_of(path)?.to_string()),
            None => None,
        };
        let tree = worktree_dir(job, root).map_err(bad)?;
        let calls = plan_prepare(&facts, job, root, bundle_text.as_deref()).map_err(bad)?;
        let dir = repo_dir(&facts, root).map_err(bad)?;
        let _turn = self.turns.claim(&dir);
        let existed = std::fs::symlink_metadata(&tree).is_ok();
        let result = run_calls(self.git.as_ref(), &calls);
        if let Some(t) = transfer {
            self.blobs.discard(t);
        }
        if let Err(why) = result {
            if !existed {
                if let Ok(undo) = plan_cleanup(&facts, job, root) {
                    let _ = run_calls(self.git.as_ref(), &undo);
                }
            }
            return Err(failed(why));
        }
        Ok(json!({"cwd": tree, "head": facts.head}))
    }

    pub fn give_back(&self, args: &Value) -> Result<Value, ApiError> {
        let job = args
            .get("job")
            .and_then(Value::as_str)
            .filter(|j| safe_name(j))
            .ok_or_else(|| bad("job is a short name"))?;
        let from = args
            .get("from")
            .and_then(Value::as_u64)
            .ok_or_else(|| bad("from is a number"))?;
        let out_dir = private_dir(&self.cluster, OUTGOING_DIR)
            .map_err(|e| failed(format!("the outgoing folder: {e}")))?;
        let nonce = if from == 0 {
            hex(&random_bytes(8).map_err(|_| failed("no randomness for the return"))?)
        } else {
            args.get("nonce")
                .and_then(Value::as_str)
                .filter(|n| safe_name(n))
                .ok_or_else(|| bad("nonce is the one the first reply gave"))?
                .to_string()
        };
        let file = out_dir.join(format!("{job}.{nonce}.return.bundle"));
        if from == 0 {
            sweep_old(&out_dir, SystemTime::now(), RETURN_SUFFIX);
            let root = self.work_root()?;
            let call = plan_return(job, text_of(&root)?, text_of(&file)?).map_err(bad)?;
            if let Err(why) = run_calls(self.git.as_ref(), &[call]) {
                remove_plain(&file);
                return Err(failed(why));
            }
        }
        if !is_real_file(&file) {
            return Err(failed("there is no work waiting to be brought back"));
        }
        let mut open = std::fs::File::open(&file).map_err(|e| failed(e.to_string()))?;
        let size = open.metadata().map_err(|e| failed(e.to_string()))?.len();
        if from > size {
            return Err(bad("from is past the end of the file"));
        }
        let want = (size - from).min(CHUNK as u64) as usize;
        let mut data = vec![0; want];
        open.seek(SeekFrom::Start(from))
            .and_then(|_| open.read_exact(&mut data))
            .map_err(|e| failed(e.to_string()))?;
        let next = from + want as u64;
        let done = next == size;
        let mut reply = json!({
            "data": base64(&data),
            "next": next,
            "size": size,
            "done": done,
            "nonce": nonce,
        });
        if done {
            reply["sha256"] = json!(hash_file(&file).map_err(|e| failed(e.to_string()))?);
            remove_plain(&file);
        }
        Ok(reply)
    }

    pub fn call(&self, op: &str, args: &Value) -> Option<Result<Value, ApiError>> {
        match op {
            NODE_PUT => Some(self.blobs.put(args)),
            NODE_PUT_END => Some(self.blobs.end(args)),
            NODE_PREPARE => Some(self.prepare(args)),
            NODE_RETURN => Some(self.give_back(args)),
            _ => None,
        }
    }
}

pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut digest = Digest::default();
    let mut block = vec![0; CHUNK];
    loop {
        let n = file.read(&mut block)?;
        if n == 0 {
            return Ok(digest.finish());
        }
        digest.update(&block[..n]);
    }
}

pub struct WorkspaceCalls {
    inner: Arc<dyn LocalCalls>,
    workspace: Arc<Workspace>,
}

impl WorkspaceCalls {
    pub fn new(inner: Arc<dyn LocalCalls>, workspace: Arc<Workspace>) -> WorkspaceCalls {
        WorkspaceCalls { inner, workspace }
    }
}

impl LocalCalls for WorkspaceCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        match self.workspace.call(op, args) {
            Some(result) => result,
            None => self.inner.call(op, args, origin),
        }
    }
}

#[cfg(test)]
#[path = "cluster_workspace_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cluster_workspace_turns_tests.rs"]
mod turns_tests;
