use crate::acting_ops::{bad, forbidden, Room};
use crate::switches::{SwitchFlag, INSTALL_OFF};
use crate::versions_ops::Clock;
use notchling_engine::control::launcher::Launcher;
use notchling_engine::control::plan::Command;
use notchling_engine::versions::latest;
use notchling_engine::versions::mac::Installed;
use notchling_engine::versions::record::Cause;
use notchling_engine::versions::update::update_argv;
use notchling_engine::versions::{Tool, Version};
use notchling_local_api::{topics, ApiError};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub const LATEST_TTL_MS: i64 = 3_600_000;
pub const ERROR_TTL_MS: i64 = 300_000;
pub const CAUSE_TTL_MS: i64 = 1_800_000;
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
pub const UPDATE_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_BODY: usize = 1024 * 1024;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone)]
struct Cached {
    latest: Option<Version>,
    at: i64,
    error: Option<String>,
}

pub struct Updates {
    launcher: Arc<dyn Launcher>,
    room: Arc<dyn Room>,
    clock: Clock,
    switch: Mutex<Option<Arc<SwitchFlag>>>,
    cache: Mutex<BTreeMap<Tool, Cached>>,
    running: Mutex<BTreeSet<Tool>>,
    causes: Mutex<BTreeMap<Tool, (Cause, i64)>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    rescan: Arc<AtomicBool>,
}

impl Updates {
    pub fn new(
        launcher: Arc<dyn Launcher>,
        room: Arc<dyn Room>,
        clock: Clock,
        rescan: Arc<AtomicBool>,
    ) -> Updates {
        Updates {
            launcher,
            room,
            clock,
            switch: Mutex::new(None),
            cache: Mutex::new(BTreeMap::new()),
            running: Mutex::new(BTreeSet::new()),
            causes: Mutex::new(BTreeMap::new()),
            threads: Mutex::new(Vec::new()),
            rescan,
        }
    }

    pub fn set_switch(&self, switch: Arc<SwitchFlag>) {
        *lock(&self.switch) = Some(switch);
    }

    pub fn allowed(&self) -> Result<(), ApiError> {
        match lock(&self.switch).as_ref() {
            Some(switch) => switch.check(),
            None => Err(forbidden(INSTALL_OFF)),
        }
    }

    pub fn known_latest(&self, tool: Tool) -> Option<Version> {
        lock(&self.cache).get(&tool).and_then(|c| c.latest.clone())
    }

    pub fn latest(&self, tool: Tool) -> Value {
        let now = (self.clock)();
        let cached = lock(&self.cache).get(&tool).cloned();
        if let Some(c) = cached {
            let ttl = if c.error.is_some() {
                ERROR_TTL_MS
            } else {
                LATEST_TTL_MS
            };
            if now - c.at < ttl {
                return reply(&c);
            }
        }
        let fresh = self.fetch(tool, now);
        lock(&self.cache).insert(tool, fresh.clone());
        reply(&fresh)
    }

    fn fetch(&self, tool: Tool, now: i64) -> Cached {
        let Some(url) = latest::source(tool) else {
            return Cached {
                latest: None,
                at: now,
                error: None,
            };
        };
        let host = latest::host(&url).unwrap_or("the vendor").to_string();
        let command = Command::new(&[
            "curl",
            "--proto",
            "=https",
            "-fsSL",
            "--max-time",
            "10",
            &url,
        ]);
        let found = match self.launcher.run_capped(&command, FETCH_TIMEOUT, MAX_BODY) {
            Ok((out, false)) if out.code == 0 => latest::parse(tool, &out.output),
            _ => None,
        };
        Cached {
            latest: found.clone(),
            at: now,
            error: found.is_none().then(|| format!("couldn't reach {host}")),
        }
    }

    pub fn take_cause(&self, tool: Tool, now: i64) -> Cause {
        match lock(&self.causes).remove(&tool) {
            Some((cause, at)) if now - at <= CAUSE_TTL_MS => cause,
            _ => Cause::Outside,
        }
    }

    pub fn start(
        self: &Arc<Self>,
        tool: Tool,
        installed: &Installed,
        to: Option<Version>,
        cause: Cause,
    ) -> Result<Value, ApiError> {
        self.allowed()?;
        let argv = update_argv(tool, installed.method, to.as_ref()).map_err(bad)?;
        if !lock(&self.running).insert(tool) {
            return Ok(json!({ "started": false, "running": true }));
        }
        let command = build(tool, installed, &argv);
        lock(&self.causes).insert(tool, (cause, (self.clock)()));
        let from = installed.version.clone();
        let target = to.or_else(|| self.known_latest(tool));
        let me = self.clone();
        let handle = std::thread::spawn(move || {
            let ok = matches!(me.launcher.run(&command, UPDATE_TIMEOUT), Ok(out) if out.code == 0);
            lock(&me.running).remove(&tool);
            me.room.publish(
                topics::VERSIONS,
                topics::VERSIONS_UPDATED,
                &json!({
                    "tool": tool.name(),
                    "from": from.as_ref().map(|v| v.as_str()),
                    "to": target.as_ref().map(|v| v.as_str()),
                    "ok": ok,
                }),
            );
            me.rescan.store(true, Ordering::SeqCst);
        });
        let mut threads = lock(&self.threads);
        threads.retain(|h| !h.is_finished());
        threads.push(handle);
        Ok(json!({ "started": true, "running": false }))
    }

    pub fn wait(&self) {
        let handles: Vec<JoinHandle<()>> = std::mem::take(&mut *lock(&self.threads));
        for handle in handles {
            let _ = handle.join();
        }
    }
}

fn reply(c: &Cached) -> Value {
    let mut out = json!({
        "latest": c.latest.as_ref().map(|v| v.as_str()),
        "at": c.at,
    });
    if let Some(error) = &c.error {
        out["error"] = json!(error);
    }
    out
}

fn build(tool: Tool, installed: &Installed, argv: &[String]) -> Command {
    let first = argv.first().map(String::as_str).unwrap_or("");
    let mut command = Command::from_args(first, &argv[1..]);
    if first == tool.command() {
        command.argv[0] = installed.path.clone();
    } else if first == "npm" {
        if let Some(bin) = npm_bin(&installed.path) {
            command.argv[0] = format!("{bin}/npm");
            command = command.with_env(
                "PATH",
                Some(&format!("{bin}:/usr/bin:/bin:/usr/sbin:/sbin")),
            );
        }
    } else if first == "brew" {
        for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
            if Path::new(dir).join("brew").is_file() {
                command.argv[0] = format!("{dir}/brew");
                break;
            }
        }
    }
    command
}

fn npm_bin(path: &str) -> Option<String> {
    let at = path.find("/lib/node_modules/")?;
    Some(format!("{}/bin", &path[..at]))
}
