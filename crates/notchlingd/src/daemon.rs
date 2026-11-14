use crate::acting::{file_says_on, SETTINGS_NAME};
use crate::acting_ops::Room;
pub use crate::daemon_wiring::{Ops, Wired};
use crate::layout::Layout;
use crate::log_activity::Publish;
use crate::log_bus::wall_ms;
use crate::log_checks::InboxWarnings;
use crate::log_ops::LogOps;
use crate::log_rotate::{StderrLog, CHECK_EVERY};
use crate::log_start;
use crate::signals;
use notchling_engine::bridge::net::ports as bridge_ports;
use notchling_engine::sessions::{config_dirs, Monitor, Probes};
use notchling_events::kinds::choices::SwitchName;
use notchling_events::kinds::security::{DaemonStopped, StopCause, SwitchCause, SwitchChanged};
use notchling_events::spec::Outcome;
use notchling_events::{Emit, Event, Millis};
use notchling_local_api::lock::InstanceLock;
use notchling_local_api::tier::installed_app_paths;
use notchling_local_api::{
    topics, Allowlist, Limits, Server, ServerConfig, ServerHandle, APP_TIER_PATHS_ENV,
};
use notchling_platform::watch::{Batch, WatchConfig, Watcher};
use serde_json::Value;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const RESCAN_EVERY: Duration = Duration::from_secs(5);

pub const FIRST_LIST_WAIT: Duration = Duration::from_secs(4);

#[derive(Debug)]
pub enum DaemonError {
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

pub struct FirstScan {
    done: Mutex<bool>,
    landed: Condvar,
}

impl FirstScan {
    pub fn pending() -> Arc<FirstScan> {
        Arc::new(FirstScan {
            done: Mutex::new(false),
            landed: Condvar::new(),
        })
    }

    pub fn open() -> Arc<FirstScan> {
        let first = FirstScan::pending();
        first.finish();
        first
    }

    pub fn finish(&self) {
        *self.done.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.landed.notify_all();
    }

    pub fn wait(&self, within: Duration) -> bool {
        let guard = self.done.lock().unwrap_or_else(|e| e.into_inner());
        let (guard, _) = self
            .landed
            .wait_timeout_while(guard, within, |done| !*done)
            .unwrap_or_else(|e| e.into_inner());
        *guard
    }
}

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

struct Lister {
    layout: Layout,
    monitor: Monitor,
}

impl Lister {
    fn new(layout: Layout) -> Lister {
        let mut monitor = Monitor::default();
        if layout.fixture {
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

    fn watched(&self) -> Vec<PathBuf> {
        config_dirs(&self.layout.roots())
            .into_iter()
            .flat_map(|c| [c.dir.join("jobs"), c.dir.join("sessions")])
            .filter(|d| d.is_dir())
            .collect()
    }
}

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

#[derive(Clone)]
pub struct DaemonOptions {
    pub watch: WatchConfig,
    pub rescan_every: Duration,
    pub first_list_wait: Duration,
    pub before_scan: Option<Arc<dyn Fn() + Send + Sync>>,
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

pub fn run(
    layout: Layout,
    options: DaemonOptions,
    stop: &dyn Fn() -> bool,
) -> Result<(), DaemonError> {
    if !layout.socket_given {
        notchling_platform::files::ensure_private_dir(&layout.state_dir())
            .map_err(|e| DaemonError::Io(io::Error::other(e)))?;
    }
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
    let sessions = Arc::new(Mutex::new(Value::Array(Vec::new())));
    let first_scan = FirstScan::pending();
    let room = Arc::new(LateRoom::default());
    let publish: Publish = {
        let room = room.clone();
        Arc::new(move |topic, event, data| room.publish(topic, event, &data))
    };
    let logs = log_start::start(&layout, env!("CARGO_PKG_VERSION"), publish.clone());
    let began = Instant::now();
    let wired = Wired::build(
        &layout,
        sessions.clone(),
        room.clone(),
        options.bridge_ports,
    );
    wired.set_events(logs.bus.clone());
    let checker = logs.checker(
        Arc::new(InboxWarnings::new(wired.inbox.clone(), Arc::new(wall_ms))),
        publish,
    );
    wired.set_log(
        logs.db.clone(),
        Arc::new(LogOps::new(logs.audit.clone(), checker.clone())),
    );
    let _check_schedule = checker.spawn_schedule();
    let server = Server::bind(
        ServerConfig {
            socket: layout.socket.clone(),
            allowlist,
            events: Some(logs.bus.clone()),
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
    if file_says_on(&layout.data_dir.join(SETTINGS_NAME)) {
        logs.bus.push(Event::new(
            &SwitchChanged {
                switch: SwitchName::Acting,
                on: false,
                cause: SwitchCause::FileSaidOnIgnored,
            },
            Outcome::Refused,
            wall_ms(),
        ));
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

    let log = StderrLog::adopt(&layout.log_path());
    let mut last_log_check = Instant::now();
    if let Some(log) = &log {
        rotate_log(log);
    }
    let mut watched = lister.watched();
    let mut watch = start_watch(&watched, options.watch);
    scan(&mut lister, &options, &sessions, &handle);
    first_scan.finish();
    let mut last_poll = Instant::now();
    while !stop() {
        if let Some(log) = log
            .as_ref()
            .filter(|_| last_log_check.elapsed() >= CHECK_EVERY)
        {
            last_log_check = Instant::now();
            rotate_log(log);
        }
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
        let now = lister.watched();
        if now != watched {
            drop(watch.take());
            watch = start_watch(&now, options.watch);
            watched = now;
        }
    }
    first_scan.finish();
    wired.close();
    handle.shutdown();
    let served = match thread.join() {
        Ok(result) => result.map_err(DaemonError::Io),
        Err(_) => Err(DaemonError::Io(io::Error::other(
            "the server thread panicked",
        ))),
    };
    logs.bus.push(Event::new(
        &DaemonStopped {
            cause: if signals::stop_requested() {
                StopCause::Signal
            } else {
                StopCause::Quit
            },
            uptime_ms: Millis(u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX)),
        },
        Outcome::Info,
        wall_ms(),
    ));
    logs.bus.close(Duration::from_secs(2));
    served
}

fn rotate_log(log: &StderrLog) {
    match log.check() {
        Ok(true) => eprintln!("notchling: the log passed 5 MB; the old one is notchlingd.log.1"),
        Ok(false) => {}
        Err(e) => eprintln!("notchling: rotating the log failed ({e})"),
    }
}

fn start_watch(folders: &[PathBuf], config: WatchConfig) -> Option<(Watcher, Receiver<Batch>)> {
    if folders.is_empty() {
        return None;
    }
    Watcher::start(folders.to_vec(), config).ok()
}

fn wait_batch(rx: &Receiver<Batch>, wait: Duration) -> bool {
    match rx.recv_timeout(wait) {
        Ok(_) => true,
        Err(RecvTimeoutError::Timeout) => false,
        Err(RecvTimeoutError::Disconnected) => {
            std::thread::sleep(wait);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_local_api::{ops, Caller, ClientKind, Handler, OpSpec, Tier};
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
