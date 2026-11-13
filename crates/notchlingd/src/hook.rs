use crate::acting_ops::HOOK_WAIT;
use crate::hook_output::{is_prompt_event, output};
use crate::hook_pages::{self, PagesEnv};
use crate::layout::Layout;
use notchling_engine::pages;
use notchling_local_api::{ops, Client, ClientKind, ConnectOptions};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
pub const REPLY_SLACK: Duration = Duration::from_secs(5);
pub const ANCESTORS: usize = 6;
const MAX_INPUT: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct HookOptions {
    pub socket: PathBuf,
    pub claude_dir: PathBuf,
    pub ancestors: Vec<u32>,
    pub wait: Duration,
    pub slack: Duration,
    pub connect_timeout: Duration,
    pub check_server: bool,
}

impl HookOptions {
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

pub fn decide(input: &[u8], options: &HookOptions) -> Option<Value> {
    let event: Value = serde_json::from_slice(input).ok()?;
    decide_event(&event, options)
}

fn decide_event(event: &Value, options: &HookOptions) -> Option<Value> {
    if !is_prompt_event(event) || headless(&options.claude_dir.join("sessions"), &options.ancestors)
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
    let args = json!({
        "event": event,
        "waitMs": options.wait.as_millis() as u64,
    });
    let response = client.call(ops::HOOK_REQUEST, args).ok()?;
    output(&response, event)
}

fn answer(input: &[u8], layout: &Layout) -> Option<Value> {
    let event: Value = serde_json::from_slice(input).ok()?;
    if pages::hook::handles(&event) {
        return hook_pages::answer(event, PagesEnv::for_process(layout));
    }
    let options = HookOptions::for_process(layout.socket.clone(), layout.claude_dir.clone());
    decide_event(&event, &options)
}

pub fn run(layout: &Layout) -> i32 {
    let mut input = Vec::new();
    if std::io::stdin()
        .take(MAX_INPUT)
        .read_to_end(&mut input)
        .is_err()
    {
        return 0;
    }
    if let Some(out) = answer(&input, layout) {
        if let Ok(text) = serde_json::to_vec(&out) {
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(&text);
            let _ = stdout.flush();
        }
    }
    0
}

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
