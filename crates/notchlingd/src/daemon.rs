//! `notchling daemon`: serves the session list and its changes over the local API. Milestone 3
//! adds the acting ops and the hook's request ([`ActingOps`]), all behind the `acting` switch,
//! which is off: until the maintainer turns it on the daemon acts on nothing and owns none of the
//! resources in decision 0011.
//!
//! One [`Monitor`] polls the config folders (the main one and each lane's) on a change that
//! `platform::watch` reports under their `jobs/` and `sessions/` folders, and every few seconds
//! anyway (a process can end without touching a file, and a lane can appear). A list that differs
//! from the last one goes out as `sessions.changed` with the whole new list.
//!
//! Every area's ops are built and served by [`crate::daemon_wiring`]; this module runs them: the
//! locks, the socket, the session list and the loop.
//!
//! Start order: the single-instance locks, then the socket (so a second daemon is refused before it
//! reads anything and a client that started this one finds it answering at once), and only then the
//! first read of the list. `sessions.list` waits for that read, up to [`FIRST_LIST_WAIT`], and
//! answers with what it has after that; the read then follows as `sessions.changed`.

use crate::acting::{file_says_on, FILE_SAID_ON, SETTINGS_NAME};
use crate::acting_ops::Room;
pub use crate::daemon_wiring::{Ops, Wired};
use crate::layout::Layout;
use notchling_engine::bridge::net::ports as bridge_ports;
use notchling_engine::sessions::{config_dirs, Monitor, Probes};
use notchling_local_api::lock::InstanceLock;
use notchling_local_api::tier::installed_app_paths;
use notchling_local_api::{
    ops, topics, Allowlist, AuditEntry, AuditLog, Limits, Server, ServerConfig, ServerHandle,
    APP_TIER_PATHS_ENV,
};
use notchling_platform::watch::{Batch, WatchConfig, Watcher};
use serde_json::Value;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How often the list is read again with no change seen.
pub const RESCAN_EVERY: Duration = Duration::from_secs(5);

/// How long `sessions.list` waits for the first read of the list. Under a client's call timeout
/// ([`notchling_local_api::CALL_TIMEOUT`], 5 s), so the client hears an answer, not silence.
pub const FIRST_LIST_WAIT: Duration = Duration::from_secs(4);

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

/// Opens once the first read of the session list is stored (or failed). The daemon binds its
/// socket before that read, which on a long list or a cold disk can take longer than the 3 s a
/// client that started the daemon waits for the socket; `sessions.list` waits here instead, so
/// the answer is the real list and not an empty one.
pub struct FirstScan {
    done: Mutex<bool>,
    landed: Condvar,
}

impl FirstScan {
    /// Closed until [`FirstScan::finish`].
    pub fn pending() -> Arc<FirstScan> {
        Arc::new(FirstScan {
            done: Mutex::new(false),
            landed: Condvar::new(),
        })
    }

    /// Already open, for an `Ops` whose list is never read late.
    pub fn open() -> Arc<FirstScan> {
        let first = FirstScan::pending();
        first.finish();
        first
    }

    /// The first read is stored; wakes every `sessions.list` that waits. Also called as the daemon
    /// stops, so none outlasts it.
    pub fn finish(&self) {
        *self.done.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.landed.notify_all();
    }

    /// Waits up to `within` for the first read; true when it is in.
    pub fn wait(&self, within: Duration) -> bool {
        let guard = self.done.lock().unwrap_or_else(|e| e.into_inner());
        let (guard, _) = self
            .landed
            .wait_timeout_while(guard, within, |done| !*done)
            .unwrap_or_else(|e| e.into_inner());
        *guard
    }
}

/// The server, for the acting ops: set once it's bound, which is after they're made.
#[derive(Default)]
pub struct LateRoom(OnceLock<ServerHandle>);

impl LateRoom {
    pub fn set(&self, handle: ServerHandle) {
        let _ = self.0.set(handle);
    }
}

impl Room for LateRoom {
    fn publish(&self, topic: &str, event: &str, data: &Value) {
        if let Some(handle) = self.0.get() {
            handle.publish(topic, event, data);
        }
    }

    fn can_answer(&self) -> bool {
        self.0.get().is_some_and(ServerHandle::app_attached)
    }

    fn can_follow(&self, topic: &str) -> bool {
        self.0.get().is_some_and(|h| h.app_following(topic))
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

/// One read of the list: kept, and sent as `sessions.changed` when it differs from the last one.
/// A read that panicked changes nothing. The store comes before the send, so a client that
/// subscribed and then lists never misses it: it gets the event, the list or both.
fn scan(
    lister: &mut Lister,
    options: &DaemonOptions,
    sessions: &Mutex<Value>,
    handle: &ServerHandle,
) {
    if let Some(hold) = &options.before_scan {
        hold();
    }
    let Some(next) = poll_or_keep(|| lister.poll()) else {
        return;
    };
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

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// How the daemon watches and how long it waits; tests shorten these and can hold the first read.
#[derive(Clone)]
pub struct DaemonOptions {
    pub watch: WatchConfig,
    pub rescan_every: Duration,
    /// How long `sessions.list` waits for the first read before it answers with what it has.
    pub first_list_wait: Duration,
    /// Runs before every read of the list. A test blocks the first read here to stand in for a
    /// slow disk or a long list; nothing in the app sets it.
    pub before_scan: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The phone bridge's LAN and tunnel ports, used only once the app turns the bridge on.
    pub bridge_ports: (u16, u16),
}

impl Default for DaemonOptions {
    fn default() -> Self {
        DaemonOptions {
            watch: WatchConfig::default(),
            rescan_every: RESCAN_EVERY,
            first_list_wait: FIRST_LIST_WAIT,
            before_scan: None,
            bridge_ports: (bridge_ports::LAN, bridge_ports::TUNNEL),
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
    // Empty until the first read, which comes after the socket answers (below).
    let sessions = Arc::new(Mutex::new(Value::Array(Vec::new())));
    let first_scan = FirstScan::pending();
    let room = Arc::new(LateRoom::default());
    // Every area's ops, each off at every start whatever a file says (decision 0007's note: files
    // at rest are not a tier boundary): acting, the bridge and Milestone 6's three switches.
    let wired = Wired::build(
        &layout,
        sessions.clone(),
        room.clone(),
        options.bridge_ports,
    );
    let server = Server::bind(
        ServerConfig {
            socket: layout.socket.clone(),
            allowlist,
            audit: Some(AuditLog::new(layout.audit_path())),
            version: env!("CARGO_PKG_VERSION").to_string(),
            limits: Limits::default(),
        },
        Arc::new(wired.ops(
            &layout,
            sessions.clone(),
            first_scan.clone(),
            options.first_list_wait,
        )),
    )
    .map_err(|e| match e.kind() {
        io::ErrorKind::AddrInUse => DaemonError::AlreadyRunning(layout.socket.clone()),
        _ => DaemonError::Io(e),
    })?;
    // Bound, so the audit log's folder exists.
    if file_says_on(&layout.data_dir.join(SETTINGS_NAME)) {
        let entry = AuditEntry::now(ops::SETTINGS_SET_ACTING, None, None, FILE_SAID_ON);
        let _ = AuditLog::new(layout.audit_path()).append(&entry);
        eprintln!("notchling: {SETTINGS_NAME} says acting is on; ignored, acting starts off");
    }
    wired.load();
    let (handle, thread) = server.spawn().map_err(DaemonError::Io)?;
    room.set(handle.clone());
    wired.start();
    eprintln!(
        "notchling daemon {} serving {}",
        env!("CARGO_PKG_VERSION"),
        layout.socket.display()
    );

    // Watching starts before the first read, so a file that changes during it is read again.
    let mut watched = lister.watched();
    let mut watch = start_watch(&watched, options.watch);
    // The socket answers by now, so a slow first read holds up only `sessions.list`, and for
    // `first_list_wait` at most. Whatever it finds goes out like any change.
    scan(&mut lister, &options, &sessions, &handle);
    first_scan.finish();
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
        scan(&mut lister, &options, &sessions, &handle);
        wired.agy.reap();
        // A lane or a jobs/sessions folder that came or went: watch the new set.
        let now = lister.watched();
        if now != watched {
            drop(watch.take());
            watch = start_watch(&now, options.watch);
            watched = now;
        }
    }
    // Held hooks pass now rather than at their deadline; the bridge's ports close; a list still
    // waiting for the first read stops waiting.
    first_scan.finish();
    wired.close();
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
    use notchling_local_api::{Caller, ClientKind, Handler, OpSpec, Tier};
    use serde_json::json;

    fn caller() -> Caller {
        Caller {
            tier: Tier::Agent,
            client: ClientKind::Cli,
            peer: None,
            pid: None,
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
    fn a_list_answered_before_the_first_read_says_partial() {
        let first = FirstScan::pending();
        let ops = Ops::new(Arc::new(Mutex::new(json!([]))))
            .with_first_scan(first.clone(), Duration::from_millis(10));
        let (list, extras) = ops.call_with_extras(&caller(), ops::SESSIONS_LIST, &Value::Null);
        assert_eq!(list.unwrap(), json!([]), "the result stays a bare list");
        assert_eq!(extras.get("partial"), Some(&json!(true)));
        first.finish();
        let (_, extras) = ops.call_with_extras(&caller(), ops::SESSIONS_LIST, &Value::Null);
        assert!(extras.is_empty(), "{extras:?}");
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
    fn the_first_list_wait_ends_before_a_client_gives_up() {
        // A client's call times out at CALL_TIMEOUT: the wait must end first, so it hears an answer.
        assert!(FIRST_LIST_WAIT < notchling_local_api::CALL_TIMEOUT);
    }

    #[test]
    fn the_first_read_wakes_a_waiting_list() {
        let first = FirstScan::pending();
        assert!(!first.wait(Duration::from_millis(20)), "still pending");
        let waiter = {
            let first = first.clone();
            std::thread::spawn(move || first.wait(Duration::from_secs(20)))
        };
        std::thread::sleep(Duration::from_millis(50));
        first.finish();
        assert!(waiter.join().unwrap(), "woken by the first read");
        // Open for good, and a list with nothing to wait for doesn't wait.
        assert!(first.wait(Duration::ZERO));
        assert!(FirstScan::open().wait(Duration::ZERO));
    }

    #[test]
    fn a_list_past_its_wait_answers_with_what_the_cache_holds() {
        let ops = Ops::new(Arc::new(Mutex::new(json!([{"id": "a"}]))))
            .with_first_scan(FirstScan::pending(), Duration::from_millis(30));
        let started = Instant::now();
        let got = ops
            .call(&caller(), ops::SESSIONS_LIST, &Value::Null)
            .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(30));
        assert_eq!(got, json!([{"id": "a"}]));
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
