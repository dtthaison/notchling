use crate::acting_ops::bad;
use crate::cluster_node::DELEGATE_WAIT;
use crate::contract::{self, Failure};
use crate::delegate_cli;
use crate::layout::Layout;
use notchling_engine::cluster::shell_job::CODEX_DELEGATE;
use notchling_engine::delegate::command::{self, Context};
use notchling_engine::delegate::options::{
    self, take_machine, to_args, Parsed, Tool, MAX_REMOTE_TIMEOUT_SECS,
};
use notchling_engine::delegate::real::{self, Folders};
use notchling_local_api::{ops, ApiError, Client, ClientError, ClientKind};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

pub const OUTPUT_CAP: usize = 256 * 1024;
const NESTED_DEPTH: &str = "1";

pub trait DelegateRunner: Send + Sync {
    fn run(&self, args: &[String]) -> (i32, String, String);
}

pub struct RealDelegate {
    folders: Folders,
    stop: Arc<AtomicBool>,
}

impl RealDelegate {
    pub fn new(layout: &Layout) -> RealDelegate {
        let real_home = notchling_platform::paths::home().unwrap_or_else(|| layout.home());
        let path = std::env::var("PATH").unwrap_or_default();
        let folders = crate::delegate_cli::folders(
            layout,
            &real_home,
            path,
            std::env::var_os("CODEX_HOME").as_deref(),
        );
        RealDelegate {
            folders,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

pub fn cut(text: String) -> String {
    if text.len() <= OUTPUT_CAP {
        return text;
    }
    let mut end = OUTPUT_CAP;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[output cut at {} KB]\n",
        &text[..end],
        OUTPUT_CAP / 1024
    )
}

impl DelegateRunner for RealDelegate {
    fn run(&self, args: &[String]) -> (i32, String, String) {
        let deps = real::deps(&self.folders, self.stop.clone());
        let ctx = Context {
            cwd: Path::new("/"),
            home: Some(&self.folders.home),
            depth: Some(NESTED_DEPTH),
            record: None,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = command::run(args, &ctx, deps, &mut out, &mut err);
        let text = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).into_owned();
        (code, text(out), text(err))
    }
}

pub fn check(args: &[String]) -> Result<Parsed, ApiError> {
    let head: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .take_while(|a| *a != "--")
        .collect();
    for flag in ["--write", "--prompt-file", "--out"] {
        if head.contains(&flag) {
            return Err(bad(format!(
                "{flag} isn't allowed for a job on another machine"
            )));
        }
    }
    let parsed = options::parse(args, Path::new("/"), None).map_err(|e| bad(e.message))?;
    if parsed.options.tool == Tool::Codex {
        return Err(bad(CODEX_DELEGATE));
    }
    if !head.contains(&"--folder") {
        return Err(bad("--folder is needed for a job on another machine"));
    }
    if parsed.options.timeout > Duration::from_secs(MAX_REMOTE_TIMEOUT_SECS) {
        return Err(bad(format!(
            "--timeout is at most {MAX_REMOTE_TIMEOUT_SECS} seconds for a job on another machine"
        )));
    }
    Ok(parsed)
}

pub fn args_of(value: &Value) -> Result<Vec<String>, ApiError> {
    value
        .as_array()
        .and_then(|list| {
            list.iter()
                .map(|v| v.as_str().map(str::to_string))
                .collect::<Option<Vec<String>>>()
        })
        .ok_or_else(|| bad("args is a list of strings"))
}

pub type Call<'a> = dyn FnMut(&str, Value) -> Result<Value, ClientError> + 'a;

fn find_node(status: &Value, wanted: &str) -> Option<String> {
    let members = status.get("members").and_then(Value::as_array)?;
    let lower = wanted.to_lowercase();
    members
        .iter()
        .filter(|m| m.get("state").and_then(Value::as_str) == Some("active"))
        .find(|m| {
            m.get("id").and_then(Value::as_str) == Some(wanted)
                || m.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|n| n.to_lowercase() == lower)
        })
        .and_then(|m| m.get("id").and_then(Value::as_str).map(str::to_string))
}

pub fn run_remote(
    machine: &str,
    rest: &[String],
    (cwd, home): (&Path, Option<&Path>),
    call: &mut Call<'_>,
    (out, err): (&mut dyn Write, &mut dyn Write),
) -> i32 {
    let json = contract::wants_json(rest);
    let parsed = match options::parse(rest, cwd, home) {
        Ok(p) => p,
        Err(e) => return Failure::usage(e.message).emit(json, out, err),
    };
    let list = to_args(&parsed);
    if let Err(e) = check(&list) {
        return Failure::usage(e.message).emit(json, out, err);
    }
    let status = match call(ops::CLUSTER_STATUS, json!({})) {
        Ok(status) => status,
        Err(e) => return Failure::from_client(Some(ops::CLUSTER_STATUS), e).emit(json, out, err),
    };
    let Some(node) = find_node(&status, machine) else {
        return Failure::new(
            contract::BAD_REQUEST,
            "bad_value",
            "No active machine has that name.",
        )
        .with_human(format!("No node named {machine}."))
        .with_op(ops::CLUSTER_DISPATCH)
        .emit(json, out, err);
    };
    let key = notchling_engine::vault::random_id("d-").unwrap_or_else(|| {
        format!(
            "d-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        )
    });
    let reply = match call(
        ops::CLUSTER_DISPATCH,
        json!({"node": node, "delegate": list, "key": key}),
    ) {
        Ok(reply) => reply,
        Err(e) => return Failure::from_client(Some(ops::CLUSTER_DISPATCH), e).emit(json, out, err),
    };
    if let (Some(warning), false) = (reply.get("warning").and_then(Value::as_str), json) {
        let _ = writeln!(err, "{warning}");
    }
    let Some(code) = reply.get("exit").and_then(Value::as_i64) else {
        let job = reply.get("job").and_then(Value::as_str).unwrap_or("");
        let state = reply.get("state").and_then(Value::as_str).unwrap_or("");
        return Failure::failed("The job has not finished.")
            .with_human(format!("The job {job} is {state}."))
            .with_op(ops::CLUSTER_DISPATCH)
            .emit(json, out, err);
    };
    let text = |name: &str| reply.get(name).and_then(Value::as_str).unwrap_or("");
    delegate_cli::conclude(
        i32::try_from(code).unwrap_or(contract::FAILED),
        (text("out"), text("err")),
        json,
        (out, err),
    )
}

pub fn run_machine(args: &[String]) -> Option<i32> {
    let json = contract::wants_json(args);
    let (machine, rest) = match take_machine(args) {
        Ok(taken) => taken,
        Err(e) => return Some(Failure::usage(e.message).report(json)),
    };
    let machine = machine?;
    let Some(layout) = Layout::from_env(None) else {
        return Some(Failure::not_running().report(json));
    };
    let Ok(cwd) = std::env::current_dir() else {
        return Some(Failure::failed("Can't tell which folder this runs in.").report(json));
    };
    let home = notchling_platform::paths::home().unwrap_or_else(|| layout.home());
    let mut client =
        match Client::connect(&layout.socket, ClientKind::Cli, env!("CARGO_PKG_VERSION")) {
            Ok(client) => client,
            Err(e) => return Some(Failure::from_client(None, e).report(json)),
        };
    client.set_timeout(DELEGATE_WAIT + Duration::from_secs(60));
    let mut call = |op: &str, args: Value| client.call(op, args);
    Some(run_remote(
        &machine,
        &rest,
        (&cwd, Some(&home)),
        &mut call,
        (&mut std::io::stdout(), &mut std::io::stderr()),
    ))
}

#[cfg(test)]
#[path = "cluster_dispatch_delegate_tests.rs"]
mod tests;
