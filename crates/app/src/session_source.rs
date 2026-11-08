//! Where the session rows come from: the daemon, else the files (decision 0004, Milestone 2).
//!
//! One [`SessionSource`] serves Chat's sidebar and the notch. A background thread connects to
//! `notchlingd` over the local API, subscribes to `sessions`, lists, then follows
//! `sessions.changed`; the UI threads only ever read the latest answer ([`SessionSource::view`]),
//! which is a lock and a clone, never the socket. While there is no answer (the daemon isn't there,
//! is starting, or went away) [`View::agents`] is `None` and the caller reads the files itself, as
//! Milestone 1 did; the thread keeps trying to reconnect with a growing wait, at most
//! [`Backoff::max`] (10 s) apart, and the callers go back to the daemon's rows as soon as it is up.
//!
//! Before saying hello the client checks who serves the socket (`server_trusted`: this user, and
//! on macOS a `notchling` in an app bundle or a cargo target folder). Anything else hears nothing;
//! the files are read and both windows say "Daemon not trusted" ([`Link::Untrusted`]).
//!
//! Only the session list comes from the daemon. Transcripts and Codex rollouts are still read by
//! the app, read-only.
//!
//! The daemon is started at most once per run, from the `notchling` binary next to the app's own
//! (`ensure_daemon` refuses anything outside an app bundle or a cargo target folder). A fixture
//! root (`--data-root`, `NOTCHLING_DATA_ROOT`) and every render read files only: [`config_for`]
//! gives them no configuration, so no socket is touched and nothing is started.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notchling_engine::sessions::Agent;
use notchling_local_api::{
    ensure_daemon, topics, Client, ClientError, ClientKind, ConnectOptions, Started, SOCKET_NAME,
};

use crate::chat::data::DataRoot;

/// Starts a daemon for a socket; swapped in tests.
pub type Starter = Arc<dyn Fn(&Path) -> Result<Started, ClientError> + Send + Sync>;

/// How long to wait between connection attempts: `first`, doubling, never over `max`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
    pub first: Duration,
    pub max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff {
            first: Duration::from_secs(1),
            max: Duration::from_secs(10),
        }
    }
}

impl Backoff {
    /// The wait after `attempt` failures in a row (0 for the first).
    pub fn delay(&self, attempt: u32) -> Duration {
        self.first
            .checked_mul(1u32.checked_shl(attempt).unwrap_or(u32::MAX))
            .unwrap_or(self.max)
            .min(self.max)
    }
}

/// What the background thread needs.
#[derive(Clone)]
pub struct SourceConfig {
    pub socket: PathBuf,
    /// Tried once, when the first connect finds nobody listening. `None` never starts a daemon.
    pub start: Option<Starter>,
    pub backoff: Backoff,
    pub version: String,
    /// Check that a Notchling daemon serves the socket before saying anything (the local API's
    /// `server_trusted`). Always on in the app; off only for tests that serve from their own binary.
    pub check_server: bool,
}

/// The configuration for the running app over `root`, or `None` when it must read files only: a
/// fixture root never touches a daemon. `exe` is the running executable; the daemon to start is
/// the `notchling` beside it.
pub fn config_for(root: &DataRoot, exe: Option<&Path>) -> Option<SourceConfig> {
    if root.fixture.is_some() {
        return None;
    }
    let start = exe.and_then(Path::parent).map(|dir| {
        let binary = dir.join(format!("notchling{}", std::env::consts::EXE_SUFFIX));
        let starter: Starter = Arc::new(move |socket| ensure_daemon(&binary, socket));
        starter
    });
    Some(SourceConfig {
        socket: root.data_dir.join(SOCKET_NAME),
        start,
        backoff: Backoff::default(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        check_server: true,
    })
}

/// How the rows are being had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// By design: a fixture or a render. No daemon, and nothing to say about it.
    Files,
    /// Not yet known: the first attempt is still running.
    Connecting,
    /// From the daemon.
    Daemon,
    /// The daemon is unreachable: the files are read, and the UI says so.
    Down,
    /// Something that isn't a trusted Notchling daemon serves the socket: nothing is said to it,
    /// the files are read, and the UI says so.
    Untrusted,
}

/// The latest answer.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// The daemon's rows; `None` means read the files.
    pub agents: Option<Vec<Agent>>,
    pub link: Link,
    /// Counts every change of rows or link, so a caller can tell something moved.
    pub generation: u64,
}

impl View {
    /// Whether the "Daemon not running" note shows.
    pub fn daemon_down(&self) -> bool {
        self.link == Link::Down
    }

    /// The note both windows show about the daemon, if any.
    pub fn note(&self) -> Option<&'static str> {
        match self.link {
            Link::Down => Some(DAEMON_DOWN_NOTE),
            Link::Untrusted => Some(DAEMON_UNTRUSTED_NOTE),
            Link::Files | Link::Connecting | Link::Daemon => None,
        }
    }
}

/// The note both windows show while the daemon can't be reached.
pub const DAEMON_DOWN_NOTE: &str = "Daemon not running";

/// The note both windows show while the socket is served by something that isn't Notchling's
/// daemon.
pub const DAEMON_UNTRUSTED_NOTE: &str = "Daemon not trusted";

struct State {
    agents: Option<Vec<Agent>>,
    link: Link,
    generation: u64,
}

struct Shared {
    state: Mutex<State>,
    stop: AtomicBool,
}

impl Shared {
    fn set(&self, link: Link, agents: Option<Vec<Agent>>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.link != link || state.agents != agents {
            state.link = link;
            state.agents = agents;
            state.generation += 1;
        }
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

/// The shared source. Dropping it tells the thread to end.
pub struct SessionSource {
    shared: Arc<Shared>,
}

impl SessionSource {
    /// A source with no daemon behind it: [`Link::Files`] always.
    pub fn files() -> SessionSource {
        SessionSource::with_link(Link::Files)
    }

    fn with_link(link: Link) -> SessionSource {
        SessionSource {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    agents: None,
                    link,
                    generation: 0,
                }),
                stop: AtomicBool::new(false),
            }),
        }
    }

    /// Starts the background thread; callers read files until it has the daemon's answer.
    pub fn start(config: SourceConfig) -> SessionSource {
        let source = SessionSource::with_link(Link::Connecting);
        let shared = source.shared.clone();
        let spawned = std::thread::Builder::new()
            .name("notchling-sessions".into())
            .spawn(move || run(config, &shared));
        if spawned.is_err() {
            source.shared.set(Link::Down, None);
        }
        source
    }

    /// The source for the running app over `root`: the daemon's, or files only when `root` is a
    /// fixture (see [`config_for`]).
    pub fn for_run(root: Option<&DataRoot>, exe: Option<&Path>) -> SessionSource {
        match root.and_then(|root| config_for(root, exe)) {
            Some(config) => SessionSource::start(config),
            None => SessionSource::files(),
        }
    }

    /// The latest answer; never waits on the socket.
    pub fn view(&self) -> View {
        let state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        View {
            agents: state.agents.clone(),
            link: state.link,
            generation: state.generation,
        }
    }

    /// The generation alone, for a poll that clones nothing.
    pub fn generation(&self) -> u64 {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .generation
    }
}

impl Drop for SessionSource {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
    }
}

/// The thread: connect, follow, and on any trouble fall back and try again later.
fn run(config: SourceConfig, shared: &Shared) {
    let mut tried_start = false;
    let mut failures = 0u32;
    while !shared.stopped() {
        let mut link = Link::Down;
        match connect(&config, &mut tried_start) {
            Ok((client, agents)) => {
                shared.set(Link::Daemon, Some(agents));
                failures = 0;
                follow(client, shared);
            }
            Err(ClientError::UntrustedServer(_)) => {
                link = Link::Untrusted;
                failures = failures.saturating_add(1);
            }
            Err(_) => failures = failures.saturating_add(1),
        }
        if shared.stopped() {
            return;
        }
        shared.set(link, None);
        // The short wait after a lost connection, longer after each failed attempt.
        sleep(config.backoff.delay(failures.saturating_sub(1)), shared);
    }
}

/// Connects (starting the daemon once if nobody listens), subscribes, then lists, so no change
/// falls between the two. Events that came in meanwhile are not older than the list, so the last
/// of them wins.
fn connect(
    config: &SourceConfig,
    tried_start: &mut bool,
) -> Result<(Client, Vec<Agent>), ClientError> {
    let options = ConnectOptions {
        check_server: config.check_server,
        ..ConnectOptions::default()
    };
    let open = || Client::connect_with(&config.socket, ClientKind::App, &config.version, &options);
    let mut client = match open() {
        Err(ClientError::NotRunning(_)) if !*tried_start && config.start.is_some() => {
            *tried_start = true;
            if let Some(start) = &config.start {
                start(&config.socket)?;
            }
            open()?
        }
        other => other?,
    };
    client.subscribe(&[topics::SESSIONS])?;
    let mut agents = client.sessions::<Agent>()?;
    while let Some(event) = client.next_event(Duration::ZERO)? {
        if event.event == topics::SESSIONS_CHANGED {
            if let Ok(newer) = serde_json::from_value(event.data) {
                agents = newer;
            }
        }
    }
    Ok((client, agents))
}

/// Applies `sessions.changed` until the connection ends or the source is dropped.
fn follow(mut client: Client, shared: &Shared) {
    while !shared.stopped() {
        match client.next_event(Duration::from_millis(250)) {
            Ok(Some(event)) if event.event == topics::SESSIONS_CHANGED => {
                if let Ok(agents) = serde_json::from_value::<Vec<Agent>>(event.data) {
                    shared.set(Link::Daemon, Some(agents));
                }
            }
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

/// Waits `total`, in short slices so a dropped source ends the thread promptly.
fn sleep(total: Duration, shared: &Shared) {
    let end = Instant::now() + total;
    while !shared.stopped() {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(Duration::from_millis(50)));
    }
}

#[cfg(test)]
#[path = "session_source_tests.rs"]
mod tests;
