//! `notchling hook`: Claude Code runs it as a hook. It reads the hook's JSON on stdin, asks the
//! daemon (`hook.request`) and prints the answer as the Swift `HookBridge` does
//! ([`crate::hook_output`]). It must never hang Claude Code, so everything that isn't a decision is
//! a pass (no output, exit 0), and at once where it can be:
//!
//! - not a permission or AskUserQuestion event;
//! - a `claude -p` session (entrypoint `sdk-cli`): nobody watches it;
//! - no daemon on the socket, or any socket error;
//! - the daemon says pass: acting is off, no app is attached, or the deadline came;
//! - its own deadline ([`HOOK_WAIT`] plus a little), whatever the daemon does.
//!
//! It is not installed anywhere in Milestone 3 (that's E13, in M6).

use crate::acting_ops::HOOK_WAIT;
use crate::hook_output::{is_prompt_event, output};
use notchling_local_api::{ops, Client, ClientKind, ConnectOptions};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Saying hello may take this long; a daemon that doesn't answer it is a pass.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// The reply may come this long after the daemon's own wait ends.
pub const REPLY_SLACK: Duration = Duration::from_secs(5);
/// How far up the process tree a session file is looked for (`isHeadlessSession`).
pub const ANCESTORS: usize = 6;
/// More stdin than any hook event: read no further.
const MAX_INPUT: u64 = 16 * 1024 * 1024;

/// Where the hook looks and how long it waits.
#[derive(Clone, Debug)]
pub struct HookOptions {
    pub socket: PathBuf,
    /// Read as `~/.claude`: its `sessions/<pid>.json` say whether a session is headless.
    pub claude_dir: PathBuf,
    /// Its parent processes, nearest first: one of them is the Claude Code session.
    pub ancestors: Vec<u32>,
    pub wait: Duration,
    pub slack: Duration,
    pub connect_timeout: Duration,
    /// Check that a Notchling daemon serves the socket (off only for tests serving from themselves).
    pub check_server: bool,
}

impl HookOptions {
    /// For this process.
    pub fn for_process(socket: PathBuf, claude_dir: PathBuf) -> HookOptions {
        HookOptions {
            socket,
            claude_dir,
            ancestors: ancestors(parent_pid_of_self(), ANCESTORS),
            wait: HOOK_WAIT,
            slack: REPLY_SLACK,
            connect_timeout: CONNECT_TIMEOUT,
            check_server: true,
        }
    }
}

/// The hook's stdout for `input`, or `None` to pass.
pub fn decide(input: &[u8], options: &HookOptions) -> Option<Value> {
    let event: Value = serde_json::from_slice(input).ok()?;
    if !is_prompt_event(&event)
        || headless(&options.claude_dir.join("sessions"), &options.ancestors)
    {
        return None;
    }
    let connect = ConnectOptions {
        timeout: options.connect_timeout,
        check_server: options.check_server,
    };
    let mut client = Client::connect_with(
        &options.socket,
        ClientKind::Hook,
        env!("CARGO_PKG_VERSION"),
        &connect,
    )
    .ok()?;
    client.set_timeout(options.wait + options.slack);
    // No pid: the daemon reads who this is from the socket.
    let args = json!({
        "event": event,
        "waitMs": options.wait.as_millis() as u64,
    });
    let response = client.call(ops::HOOK_REQUEST, args).ok()?;
    output(&response, &event)
}

/// `notchling hook`: reads stdin, prints the answer (or nothing), always exits 0.
pub fn run(socket: &Path, claude_dir: &Path) -> i32 {
    let mut input = Vec::new();
    if std::io::stdin()
        .take(MAX_INPUT)
        .read_to_end(&mut input)
        .is_err()
    {
        return 0;
    }
    let options = HookOptions::for_process(socket.to_path_buf(), claude_dir.to_path_buf());
    if let Some(out) = decide(&input, &options) {
        if let Ok(text) = serde_json::to_vec(&out) {
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(&text);
            let _ = stdout.flush();
        }
    }
    0
}

/// `isHeadlessSession`: the nearest ancestor with a `sessions/<pid>.json` decides; `sdk-cli` is a
/// `claude -p` run.
pub fn headless(sessions: &Path, ancestors: &[u32]) -> bool {
    for pid in ancestors {
        let path = sessions.join(format!("{pid}.json"));
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let session: Value = serde_json::from_slice(&data).unwrap_or(Value::Null);
        return session.get("entrypoint").and_then(Value::as_str) == Some("sdk-cli");
    }
    false
}

/// `start` and its parents, nearest first, at most `depth`, stopping at pid 1.
pub fn ancestors(start: Option<u32>, depth: usize) -> Vec<u32> {
    let mut list = Vec::new();
    let mut pid = start;
    while let Some(p) = pid.filter(|p| *p > 1) {
        if list.len() >= depth {
            break;
        }
        list.push(p);
        pid = parent_pid(p);
    }
    list
}

#[cfg(unix)]
fn parent_pid_of_self() -> Option<u32> {
    // SAFETY: getppid takes nothing and can't fail.
    u32::try_from(unsafe { libc::getppid() }).ok()
}

#[cfg(not(unix))]
fn parent_pid_of_self() -> Option<u32> {
    None
}

/// A process's parent, from the kernel.
#[cfg(target_os = "macos")]
pub fn parent_pid(pid: u32) -> Option<u32> {
    let pid = libc::c_int::try_from(pid).ok()?;
    // SAFETY: proc_bsdinfo is plain data; all zeroes is a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer is a proc_bsdinfo of the size passed.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (n == size).then_some(info.pbi_ppid)
}

/// A process's parent, from `/proc/<pid>/stat` (its fourth field, after the name in brackets).
#[cfg(target_os = "linux")]
pub fn parent_pid(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_name = stat.rsplit_once(')')?.1;
    after_name.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn parent_pid(_pid: u32) -> Option<u32> {
    None
}
