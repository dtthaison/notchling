//! `notchling daemon`: serves the session list and its changes over the local API. It only reads
//! files and acts on nothing (Milestone 2); it owns none of the resources in decision 0011.
//!
//! One [`Monitor`] polls the config folders (the main one and each lane's) on a change that
//! `platform::watch` reports under their `jobs/` and `sessions/` folders, and every few seconds
//! anyway (a process can end without touching a file, and a lane can appear). A list that differs
//! from the last one goes out as `sessions.changed` with the whole new list.

use crate::layout::Layout;
use notchling_engine::sessions::{config_dirs, Monitor, Probes};
use notchling_local_api::lock::InstanceLock;
use notchling_local_api::tier::installed_app_paths;
use notchling_local_api::{
    ops, topics, Allowlist, ApiError, AuditLog, Caller, Handler, Limits, OpSpec, Server,
    ServerConfig, APP_TIER_PATHS_ENV,
};
use notchling_platform::watch::{Batch, WatchConfig, Watcher};
use serde_json::Value;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the list is read again with no change seen.
pub const RESCAN_EVERY: Duration = Duration::from_secs(5);

/// Why the daemon didn't serve.
#[derive(Debug)]
pub enum DaemonError {
    /// Another daemon holds this socket's lock. Not a failure: exits 0.
    AlreadyRunning(PathBuf),
    Io(io::Error),
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DaemonError::AlreadyRunning(socket) => {
                write!(f, "a notchling daemon already serves {}", socket.display())
            }
            DaemonError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// The M2 ops: the session list, all agent tier.
pub struct Ops {
    sessions: Arc<Mutex<Value>>,
}

impl Ops {
    pub fn new(sessions: Arc<Mutex<Value>>) -> Ops {
        Ops { sessions }
    }
}

impl Handler for Ops {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::SESSIONS_LIST => Some(OpSpec::AGENT),
            _ => None,
        }
    }

    fn call(&self, _caller: &Caller, op: &str, _args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::SESSIONS_LIST => Ok(self
                .sessions
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn topics(&self) -> &[&str] {
        &[topics::SESSIONS]
    }
}

/// The session list now, as JSON with the engine's field names.
struct Lister {
    layout: Layout,
    monitor: Monitor,
}

impl Lister {
    fn new(layout: Layout) -> Lister {
        let mut monitor = Monitor::default();
        if layout.fixture {
            // A fixture's dates are fixed: keep its finished rows, as the app's renders do.
            monitor.completed_window_ms = i64::MAX / 4;
        }
        Lister { layout, monitor }
    }

    fn poll(&mut self) -> Value {
        let agents = self
            .monitor
            .poll(&self.layout.roots(), &Probes::from_proc(), now_ms());
        serde_json::to_value(agents).unwrap_or(Value::Array(Vec::new()))
    }

    /// The `jobs/` and `sessions/` folders that exist now, across the main folder and the lanes.
    fn watched(&self) -> Vec<PathBuf> {
        config_dirs(&self.layout.roots())
            .into_iter()
            .flat_map(|c| [c.dir.join("jobs"), c.dir.join("sessions")])
            .filter(|d| d.is_dir())
            .collect()
    }
}

/// Runs one read of the session list; `None` when it panicked, so the caller keeps serving the
/// last good list. A bug in a reader (a file shaped in a way nobody expected) must not take the
/// daemon down for every client.
fn poll_or_keep(poll: impl FnOnce() -> Value) -> Option<Value> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(poll)) {
        Ok(list) => Some(list),
        Err(payload) => {
            let why = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "no message".into());
            eprintln!("notchling: reading the sessions failed ({why}); serving the last list");
            None
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// How the daemon watches; tests shorten it.
#[derive(Clone, Copy, Debug)]
pub struct DaemonOptions {
    pub watch: WatchConfig,
    pub rescan_every: Duration,
}

impl Default for DaemonOptions {
    fn default() -> Self {
        DaemonOptions {
            watch: WatchConfig::default(),
            rescan_every: RESCAN_EVERY,
        }
    }
}

/// Serves until `stop()` says so, then shuts the server down and removes the socket.
pub fn run(
    layout: Layout,
    options: DaemonOptions,
    stop: &dyn Fn() -> bool,
) -> Result<(), DaemonError> {
    if !layout.socket_given {
        notchling_platform::files::ensure_private_dir(&layout.state_dir())
            .map_err(|e| DaemonError::Io(io::Error::other(e)))?;
    }
    // One daemon per real data folder, wherever its socket is (the socket's own lock comes with
    // `Server::bind`). Held until this returns.
    let _data_lock = match layout.data_lock() {
        Some(path) => {
            notchling_platform::files::ensure_private_dir(&layout.data_dir)
                .map_err(|e| DaemonError::Io(io::Error::other(e)))?;
            match InstanceLock::try_acquire(&path).map_err(DaemonError::Io)? {
                Some(lock) => Some(lock),
                None => return Err(DaemonError::AlreadyRunning(layout.data_dir.clone())),
            }
        }
        None => None,
    };
    let allowlist = Allowlist::for_daemon(
        &installed_app_paths(),
        std::env::var_os(APP_TIER_PATHS_ENV).as_deref(),
        std::env::current_exe().ok().as_deref(),
    );
    let mut lister = Lister::new(layout.clone());
    let first = poll_or_keep(|| lister.poll()).unwrap_or_else(|| Value::Array(Vec::new()));
    let sessions = Arc::new(Mutex::new(first));
    let server = Server::bind(
        ServerConfig {
            socket: layout.socket.clone(),
            allowlist,
            audit: Some(AuditLog::new(layout.audit_path())),
            version: env!("CARGO_PKG_VERSION").to_string(),
            limits: Limits::default(),
        },
        Arc::new(Ops::new(sessions.clone())),
    )
    .map_err(|e| match e.kind() {
        io::ErrorKind::AddrInUse => DaemonError::AlreadyRunning(layout.socket.clone()),
        _ => DaemonError::Io(e),
    })?;
    let (handle, thread) = server.spawn().map_err(DaemonError::Io)?;
    eprintln!(
        "notchling daemon {} serving {}",
        env!("CARGO_PKG_VERSION"),
        layout.socket.display()
    );

    let mut watched = lister.watched();
    let mut watch = start_watch(&watched, options.watch);
    let mut last_poll = Instant::now();
    while !stop() {
        let changed = match &watch {
            Some((_, rx)) => wait_batch(rx, Duration::from_millis(100)),
            None => {
                std::thread::sleep(Duration::from_millis(100));
                false
            }
        };
        if !changed && last_poll.elapsed() < options.rescan_every {
            continue;
        }
        last_poll = Instant::now();
        if let Some(next) = poll_or_keep(|| lister.poll()) {
            let differs = {
                let mut current = sessions.lock().unwrap_or_else(|e| e.into_inner());
                let differs = *current != next;
                if differs {
                    *current = next.clone();
                }
                differs
            };
            if differs {
                handle.publish(topics::SESSIONS, topics::SESSIONS_CHANGED, &next);
            }
        }
        // A lane or a jobs/sessions folder that came or went: watch the new set.
        let now = lister.watched();
        if now != watched {
            drop(watch.take());
            watch = start_watch(&now, options.watch);
            watched = now;
        }
    }
    handle.shutdown();
    match thread.join() {
        Ok(result) => result.map_err(DaemonError::Io),
        Err(_) => Err(DaemonError::Io(io::Error::other(
            "the server thread panicked",
        ))),
    }
}

fn start_watch(folders: &[PathBuf], config: WatchConfig) -> Option<(Watcher, Receiver<Batch>)> {
    if folders.is_empty() {
        return None;
    }
    Watcher::start(folders.to_vec(), config).ok()
}

/// True when a batch arrived within `wait`.
fn wait_batch(rx: &Receiver<Batch>, wait: Duration) -> bool {
    match rx.recv_timeout(wait) {
        Ok(_) => true,
        Err(RecvTimeoutError::Timeout) => false,
        // The watcher ended; the periodic poll carries on and a rescan restarts it.
        Err(RecvTimeoutError::Disconnected) => {
            std::thread::sleep(wait);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_local_api::{ClientKind, Tier};
    use serde_json::json;

    fn caller() -> Caller {
        Caller {
            tier: Tier::Agent,
            client: ClientKind::Cli,
            peer: None,
        }
    }

    #[test]
    fn every_m2_op_is_agent_tier_and_nothing_else_exists() {
        let ops = Ops::new(Arc::new(Mutex::new(json!([]))));
        assert_eq!(ops.spec(ops::SESSIONS_LIST), Some(OpSpec::AGENT));
        assert_eq!(ops.spec("approval.answer"), None);
        assert_eq!(ops.spec("settings.set"), None);
        assert_eq!(ops.topics(), &["sessions"]);
    }

    #[test]
    fn the_list_is_the_cached_one() {
        let list = Arc::new(Mutex::new(json!([{"id": "a"}])));
        let ops = Ops::new(list.clone());
        let got = ops
            .call(&caller(), ops::SESSIONS_LIST, &Value::Null)
            .unwrap();
        assert_eq!(got, json!([{"id": "a"}]));
    }

    #[cfg(unix)]
    #[test]
    fn a_second_daemon_on_another_socket_over_the_same_data_folder_is_refused() {
        // A stand-in for the real folders, in /tmp (socket paths are short); never the real ones.
        let root = PathBuf::from(format!("/tmp/nl-datalock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("data")).unwrap();
        let layout = Layout {
            claude_dir: root.join("claude"),
            data_dir: root.join("data"),
            socket: root.join("elsewhere/x.sock"),
            socket_given: true,
            fixture: false,
        };
        // The first daemon, as far as the data folder knows.
        let _first = InstanceLock::try_acquire(&root.join("data/notchlingd.lock"))
            .unwrap()
            .unwrap();
        let result = run(layout, DaemonOptions::default(), &|| true);
        assert!(
            matches!(result, Err(DaemonError::AlreadyRunning(_))),
            "{result:?}"
        );
        assert!(!root.join("elsewhere/x.sock").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_read_that_panics_keeps_the_last_list() {
        assert_eq!(poll_or_keep(|| json!([1])), Some(json!([1])));
        let kept = poll_or_keep(|| panic!("a reader bug"));
        assert_eq!(kept, None);
    }

    #[test]
    fn a_fixture_lister_watches_only_jobs_and_sessions() {
        let root = std::env::temp_dir().join(format!("nl-lister-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("claude/jobs")).unwrap();
        std::fs::create_dir_all(root.join("claude/projects")).unwrap();
        let layout = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
        let lister = Lister::new(layout);
        assert_eq!(lister.watched(), vec![root.join("claude/jobs")]);
        assert!(lister.monitor.completed_window_ms > 1_000_000_000_000);
        let _ = std::fs::remove_dir_all(&root);
    }
}
