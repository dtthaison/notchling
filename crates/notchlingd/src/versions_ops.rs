use crate::acting_ops::Room;
use crate::op_group::OpGroup;
use crate::versions_check::Checks;
use notchling_engine::control::launcher::{LaunchOutput, Launcher};
use notchling_engine::control::plan::Command;
use notchling_engine::machines::versions_facts;
use notchling_engine::store;
use notchling_engine::versions::mac::{detect, watch_paths, Installed, Problem, Roots};
use notchling_engine::versions::record::{Cause, Fact, Recorder};
use notchling_engine::versions::{cli_output, IdentityCache, Method, Tool, Version, WorkerTools};
use notchling_local_api::{ops, topics, ApiError, AuditEntry, AuditLog, Caller, OpSpec};
use notchling_platform::watch::{WatchConfig, Watcher};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const WORKERS_DOC: &str = versions_facts::DOC;
pub const SEEN_DOC: &str = "versions/seen";
pub const RESCAN_EVERY: Duration = Duration::from_secs(600);
pub const AGY_ASK_EVERY_MS: i64 = 600_000;
pub const AGY_TIMEOUT: Duration = Duration::from_secs(5);
const SEEN_KEEP: usize = 400;

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub struct AuditRecorder {
    log: AuditLog,
}

impl AuditRecorder {
    pub fn new(log: AuditLog) -> AuditRecorder {
        AuditRecorder { log }
    }
}

impl Recorder for AuditRecorder {
    fn record(&self, fact: Fact) {
        let (op, result) = fact.audit_words();
        let _ = self.log.append(&AuditEntry::now(op, None, None, &result));
    }
}

#[derive(Default)]
struct AgyMemo {
    version: Option<Version>,
    sha: Option<String>,
    asked_at: Option<i64>,
}

#[derive(Default)]
struct State {
    tools: Option<BTreeMap<Tool, Installed>>,
    agy: AgyMemo,
}

pub struct VersionsOps {
    roots: Roots,
    data_dir: PathBuf,
    room: Arc<dyn Room>,
    launcher: Arc<dyn Launcher>,
    recorder: Arc<dyn Recorder>,
    clock: Clock,
    own: Version,
    checks: Arc<Checks>,
    exe: Mutex<Option<PathBuf>>,
    state: Mutex<State>,
    cache: Mutex<IdentityCache>,
    gate: Mutex<()>,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn installed_json(found: &Installed) -> Value {
    json!({
        "version": found.version.as_ref().map(|v| v.as_str()),
        "sha256": found.identity.as_ref().map(|i| i.sha256.as_str()),
    })
}

fn row(tool: Tool, found: Option<&Installed>, checks: &Checks) -> Value {
    let (reference, usable, failed, check) = checks.parts(tool, found);
    let Some(found) = found else {
        return json!({
            "tool": tool.name(),
            "installed": null,
            "method": null,
            "path": null,
            "problems": [],
            "running": [],
            "reference": reference,
            "usable": false,
            "failed": failed,
            "check": check,
        });
    };
    json!({
        "tool": tool.name(),
        "installed": installed_json(found),
        "method": found.method,
        "path": found.path,
        "problems": found.problems,
        "running": [],
        "reference": reference,
        "usable": usable,
        "failed": failed,
        "check": check,
    })
}

impl VersionsOps {
    pub fn new(
        roots: Roots,
        data_dir: PathBuf,
        room: Arc<dyn Room>,
        launcher: Arc<dyn Launcher>,
        recorder: Arc<dyn Recorder>,
        clock: Clock,
        own: Version,
    ) -> Arc<VersionsOps> {
        let checks = Checks::new(
            launcher.clone(),
            roots.home.clone(),
            data_dir.clone(),
            room.clone(),
            recorder.clone(),
            clock.clone(),
        );
        Arc::new(VersionsOps {
            roots,
            data_dir,
            room,
            launcher,
            recorder,
            clock,
            own,
            checks,
            exe: Mutex::new(std::env::current_exe().ok()),
            state: Mutex::new(State::default()),
            cache: Mutex::new(IdentityCache::new()),
            gate: Mutex::new(()),
            stop: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        })
    }

    pub fn checks(&self) -> &Arc<Checks> {
        &self.checks
    }

    pub fn wait_checks(&self) {
        self.checks.wait();
    }

    pub fn set_exe(&self, exe: Option<PathBuf>) {
        *lock(&self.exe) = exe;
    }

    pub fn hashed(&self) -> u64 {
        lock(&self.cache).hashed()
    }

    pub fn rescan(&self) {
        self.scan(true);
    }

    fn scan(&self, ask_agy: bool) {
        let _gate = lock(&self.gate);
        let mut cache = std::mem::take(&mut *lock(&self.cache));
        let now = (self.clock)();
        let mut found: BTreeMap<Tool, Installed> = BTreeMap::new();
        for tool in Tool::ALL {
            let seen = match tool {
                Tool::Notchling => self.own_install(&mut cache),
                Tool::Agy => detect(&self.roots, tool, &mut cache)
                    .map(|installed| self.with_agy_version(installed, ask_agy, now)),
                other => detect(&self.roots, other, &mut cache),
            };
            if let Some(seen) = seen {
                found.insert(tool, seen);
            }
        }
        *lock(&self.cache) = cache;
        let before = lock(&self.state).tools.replace(found.clone());
        self.announce(before, &found);
        self.note_seen(&found);
        self.auto_check(&found);
    }

    fn auto_check(&self, found: &BTreeMap<Tool, Installed>) {
        for (tool, installed) in found {
            if self.checks.wanted(*tool, installed) {
                self.checks.request(*tool, installed, false);
            }
        }
    }

    pub fn retry_due_checks(&self) {
        let due = self.checks.due_retries();
        if due.is_empty() {
            return;
        }
        let found = self.snapshot();
        for tool in due {
            if let Some(installed) = found.get(&tool) {
                self.checks.request(tool, installed, false);
            }
        }
    }

    fn check(&self, args: &Value) -> Result<Value, ApiError> {
        let name = args
            .get("tool")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("tool is a name"))?;
        let tool = Tool::from_name(name).ok_or_else(|| ApiError::bad_request("unknown tool"))?;
        if !self.checks.enabled() {
            return Ok(json!({ "started": false, "running": false }));
        }
        let found = self.snapshot();
        let Some(installed) = found.get(&tool).filter(|i| i.version.is_some()) else {
            return Ok(json!({ "started": false, "running": false }));
        };
        if self.checks.is_running(tool) {
            return Ok(json!({ "started": false, "running": true }));
        }
        let started = self.checks.request(tool, installed, true);
        Ok(json!({ "started": started, "running": !started && self.checks.is_running(tool) }))
    }

    fn own_install(&self, cache: &mut IdentityCache) -> Option<Installed> {
        let exe = lock(&self.exe).clone();
        Some(Installed {
            tool: Tool::Notchling,
            version: Some(self.own.clone()),
            method: Method::Release,
            path: exe
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            identity: exe.as_deref().and_then(|p| cache.identity(p)),
            problems: Vec::new(),
        })
    }

    fn with_agy_version(&self, mut installed: Installed, ask: bool, now: i64) -> Installed {
        let sha = installed.identity.as_ref().map(|i| i.sha256.clone());
        let (known, asked_at) = {
            let state = lock(&self.state);
            let memo = &state.agy;
            let known = memo
                .version
                .clone()
                .filter(|_| memo.sha.is_some() && memo.sha == sha);
            (known, memo.asked_at)
        };
        installed.version = known;
        let due = asked_at.is_none_or(|at| now - at >= AGY_ASK_EVERY_MS);
        if ask && due {
            let version = self.ask_agy(&installed.path);
            lock(&self.state).agy = AgyMemo {
                version: version.clone(),
                sha,
                asked_at: Some(now),
            };
            installed.version = version;
        } else if asked_at.is_none() {
            return installed;
        }
        if installed.version.is_none() {
            installed.problems.push(Problem::CantTell);
        }
        installed
    }

    fn ask_agy(&self, path: &str) -> Option<Version> {
        let command = Command::new(&[path, "--version"]);
        match self.launcher.run(&command, AGY_TIMEOUT) {
            Ok(LaunchOutput {
                code: 0, output, ..
            }) => cli_output(Tool::Agy, &output),
            _ => None,
        }
    }

    fn announce(&self, before: Option<BTreeMap<Tool, Installed>>, now: &BTreeMap<Tool, Installed>) {
        let Some(before) = before else {
            return;
        };
        for tool in Tool::ALL {
            let (old, new) = (before.get(&tool), now.get(&tool));
            if old.map(|i| (&i.version, &i.identity)) == new.map(|i| (&i.version, &i.identity)) {
                continue;
            }
            self.room.publish(
                topics::VERSIONS,
                topics::VERSIONS_CHANGED,
                &json!({
                    "tool": tool.name(),
                    "installed": new.map(installed_json),
                }),
            );
            let from = old.and_then(|i| i.version.clone());
            let to = new.and_then(|i| i.version.clone());
            let both_known = from.is_some() && to.is_some();
            if from != to && (old.is_none() || new.is_none() || both_known) {
                self.recorder.record(Fact::Changed {
                    tool,
                    machine: "mac".to_string(),
                    from,
                    to,
                    method: new.or(old).map_or(Method::Unknown, |i| i.method),
                    cause: Cause::Outside,
                });
            }
        }
    }

    fn note_seen(&self, found: &BTreeMap<Tool, Installed>) {
        let keys: Vec<(Tool, Version, String)> = found
            .iter()
            .filter_map(|(tool, i)| {
                i.version
                    .clone()
                    .map(|v| (*tool, v.clone(), format!("{}:{}", tool.name(), v)))
            })
            .collect();
        if keys.is_empty() {
            return;
        }
        let mut fresh: Vec<(Tool, Version)> = Vec::new();
        let result = store::update_doc(&self.data_dir, SEEN_DOC, |old| {
            let mut list: Vec<String> = old
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            for (tool, version, key) in &keys {
                if !list.contains(key) {
                    list.push(key.clone());
                    fresh.push((*tool, version.clone()));
                }
            }
            if list.len() > SEEN_KEEP {
                let extra = list.len() - SEEN_KEEP;
                list.drain(..extra);
            }
            serde_json::to_string(&list)
                .map(Some)
                .map_err(|e| e.to_string())
        });
        if result.is_err() {
            return;
        }
        for (tool, version) in fresh {
            self.recorder.record(Fact::Seen { tool, version });
        }
    }

    fn snapshot(&self) -> BTreeMap<Tool, Installed> {
        if lock(&self.state).tools.is_none() {
            self.scan(false);
        }
        lock(&self.state).tools.clone().unwrap_or_default()
    }

    fn get(&self, args: &Value) -> Result<Value, ApiError> {
        let only = match args.get("tool") {
            None | Some(Value::Null) => None,
            Some(Value::String(name)) => {
                Some(Tool::from_name(name).ok_or_else(|| ApiError::bad_request("unknown tool"))?)
            }
            Some(_) => return Err(ApiError::bad_request("tool is a name")),
        };
        let found = self.snapshot();
        let rows: Vec<Value> = Tool::ALL
            .into_iter()
            .filter(|t| only.is_none_or(|o| o == *t))
            .map(|t| row(t, found.get(&t), &self.checks))
            .collect();
        Ok(json!({ "tools": rows }))
    }

    fn workers(&self) -> Value {
        let saved = store::doc(&self.data_dir, WORKERS_DOC)
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str::<Map<String, Value>>(&text).ok())
            .unwrap_or_default();
        let clean: Map<String, Value> = saved
            .into_iter()
            .filter_map(|(machine, value)| {
                let tools: WorkerTools = serde_json::from_value(value).ok()?;
                Some((machine, serde_json::to_value(tools).ok()?))
            })
            .collect();
        json!({ "workers": clean })
    }

    pub fn start(self: &Arc<Self>) {
        let mut slot = lock(&self.thread);
        if slot.is_some() {
            return;
        }
        let ops = self.clone();
        *slot = Some(std::thread::spawn(move || ops.run()));
    }

    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let handle = lock(&self.thread).take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
        self.checks.wait();
    }

    fn run(&self) {
        self.rescan();
        let mut last = Instant::now();
        let mut watched = watch_paths(&self.roots);
        let mut watcher = Watcher::start(watched.clone(), WatchConfig::default()).ok();
        while !self.stop.load(Ordering::SeqCst) {
            let changed = match &watcher {
                Some((_, rx)) => rx.recv_timeout(Duration::from_millis(200)).is_ok(),
                None => {
                    std::thread::sleep(Duration::from_millis(200));
                    false
                }
            };
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            self.retry_due_checks();
            if !changed && last.elapsed() < RESCAN_EVERY {
                continue;
            }
            last = Instant::now();
            self.rescan();
            let now = watch_paths(&self.roots);
            if now != watched {
                drop(watcher.take());
                watcher = Watcher::start(now.clone(), WatchConfig::default()).ok();
                watched = now;
            }
        }
    }
}

impl OpGroup for VersionsOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::VERSIONS_GET => Some(OpSpec::AGENT),
            ops::VERSIONS_WORKERS => Some(OpSpec::APP),
            ops::VERSIONS_CHECK => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::VERSIONS_GET => self.get(args),
            ops::VERSIONS_WORKERS => Ok(self.workers()),
            ops::VERSIONS_CHECK => self.check(args),
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }

    fn topics(&self) -> &[&'static str] {
        &[topics::VERSIONS]
    }
}
