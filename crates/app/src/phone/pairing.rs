//! Following the daemon's `bridge` topic (Milestone 4, U6): the app is the one who can say yes to a
//! phone.
//!
//! The daemon asks for the person's answer only when an app client follows `bridge` (an attached
//! app that doesn't would never see the request, and every phone would wait out the time to be
//! refused). So the app keeps one connection of its own: it subscribes to `bridge`, queues each
//! `bridge.pair.request` ([`Queue`], shown by the prompt window whether or not the Phone panel is
//! open) and counts each `bridge.changed` so the shell reads the bridge again at once. An answer is
//! `bridge.pair.answer {request, allow}` over a call of its own, never the following connection.
//!
//! Like the session source it reconnects with a growing wait, and a lost connection or a bridge
//! turned off clears the queue: the daemon has refused those phones already. A fixture root or a
//! render follows nothing ([`PairingFeed::none`]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notchling_local_api::{topics, Client, ClientError, ClientKind, ConnectOptions, SOCKET_NAME};
use serde_json::{json, Value};

use super::approval::{PairRequest, Prompt, Queue};
use super::bridge::now_ms;
use super::wire::{parse_status, BridgeState};
use crate::actions::{Socket, Transport};
use crate::chat::data::DataRoot;
use crate::session_source::Backoff;

/// `bridge.pair.answer`, the op the daemon takes the answer on.
const ANSWER_OP: &str = "bridge.pair.answer";

/// What the following thread needs.
#[derive(Clone)]
pub struct FeedConfig {
    pub socket: std::path::PathBuf,
    pub version: String,
    /// Check who serves the socket before saying anything. Always on in the app; off only for
    /// tests that serve from their own binary.
    pub check_server: bool,
    pub backoff: Backoff,
}

/// The configuration for the running app over `root`, or `None` for a fixture root, which never
/// touches a daemon.
pub fn config_for(root: &DataRoot) -> Option<FeedConfig> {
    root.fixture.is_none().then(|| FeedConfig {
        socket: root.data_dir.join(SOCKET_NAME),
        version: env!("CARGO_PKG_VERSION").to_string(),
        check_server: true,
        backoff: Backoff::default(),
    })
}

#[derive(Default)]
struct State {
    queue: Queue,
    /// Counts every `bridge.changed`, so the shell can tell the bridge moved.
    changes: u64,
    /// Whether a connection is following the topic now.
    following: bool,
}

struct Shared {
    state: Mutex<State>,
    stop: AtomicBool,
}

impl Shared {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn apply(&self, event: notchling_local_api::Event) {
        let mut state = self.state();
        match event.event.as_str() {
            topics::BRIDGE_PAIR_REQUEST => {
                if let Some(request) = PairRequest::from_event(&event.data) {
                    state.queue.push(request, now_ms());
                }
            }
            topics::BRIDGE_CHANGED => {
                state.changes += 1;
                // Off refuses every phone still waiting.
                if parse_status(&event.data).is_some_and(|s| s.state == BridgeState::Off) {
                    state.queue.clear();
                }
            }
            _ => {}
        }
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

/// The app's follower of the `bridge` topic. Dropping it ends the thread.
pub struct PairingFeed {
    shared: Arc<Shared>,
    transport: Option<Arc<dyn Transport>>,
}

impl PairingFeed {
    /// Follows nothing: a render's, a fixture root's.
    pub fn none() -> PairingFeed {
        PairingFeed {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                stop: AtomicBool::new(false),
            }),
            transport: None,
        }
    }

    /// The running app's: the daemon's socket, or nothing for a fixture root.
    pub fn for_run(root: Option<&DataRoot>) -> PairingFeed {
        match root.and_then(config_for) {
            Some(config) => {
                let transport = Socket {
                    socket: config.socket.clone(),
                    version: config.version.clone(),
                };
                PairingFeed::start(config, Arc::new(transport))
            }
            None => PairingFeed::none(),
        }
    }

    /// Starts following; answers go through `transport`.
    pub fn start(config: FeedConfig, transport: Arc<dyn Transport>) -> PairingFeed {
        let mut feed = PairingFeed::none();
        feed.transport = Some(transport);
        let shared = feed.shared.clone();
        let spawned = std::thread::Builder::new()
            .name("notchling-pairing".into())
            .spawn(move || run(&config, &shared));
        if let Err(e) = spawned {
            eprintln!("follow the phone bridge: {e}");
        }
        feed
    }

    /// Whether answers can be sent at all.
    pub fn is_live(&self) -> bool {
        self.transport.is_some()
    }

    /// Whether a connection follows the `bridge` topic now.
    pub fn following(&self) -> bool {
        self.shared.state().following
    }

    /// The prompt to show at `now_ms`: the oldest phone still waiting, if any. Requests the daemon
    /// has refused by now are dropped first.
    pub fn prompt(&self, now_ms: i64) -> Option<Prompt> {
        let mut state = self.shared.state();
        state.queue.expire(now_ms);
        state.queue.prompt(now_ms)
    }

    /// How many `bridge.changed` events have come in.
    pub fn bridge_changes(&self) -> u64 {
        self.shared.state().changes
    }

    /// The person's answer: `bridge.pair.answer {request, allow}`. Blocking, so the caller makes it
    /// off the UI thread. The prompt is gone at once, whatever the daemon says: a request it no
    /// longer holds (timed out, bridge off) answers `answered: false`, which is not an error.
    pub fn answer(&self, request: &str, allow: bool) -> Result<(), String> {
        self.shared.state().queue.remove(request);
        let Some(transport) = &self.transport else {
            return Err("Notchling's daemon isn't attached, so the answer can't be sent".into());
        };
        transport
            .call(ANSWER_OP, json!({"request": request, "allow": allow}))
            .map(|_: Value| ())
            .map_err(|e| match e {
                ClientError::Api(api) => api.message,
                other => other.to_string(),
            })
    }
}

impl Drop for PairingFeed {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
    }
}

/// The thread: connect, follow, and on any trouble clear what was waiting and try again later.
fn run(config: &FeedConfig, shared: &Shared) {
    let mut failures = 0u32;
    while !shared.stopped() {
        match connect(config) {
            Ok(client) => {
                failures = 0;
                shared.state().following = true;
                follow(client, shared);
            }
            Err(_) => failures = failures.saturating_add(1),
        }
        {
            let mut state = shared.state();
            state.following = false;
            // The daemon refused them when the connection went.
            state.queue.clear();
        }
        sleep(config.backoff.delay(failures.saturating_sub(1)), shared);
    }
}

fn connect(config: &FeedConfig) -> Result<Client, ClientError> {
    let options = ConnectOptions {
        check_server: config.check_server,
        ..ConnectOptions::default()
    };
    let mut client =
        Client::connect_with(&config.socket, ClientKind::App, &config.version, &options)?;
    client.subscribe(&[topics::BRIDGE])?;
    Ok(client)
}

fn follow(mut client: Client, shared: &Shared) {
    while !shared.stopped() {
        match client.next_event(Duration::from_millis(250)) {
            Ok(Some(event)) => shared.apply(event),
            Ok(None) => {}
            Err(_) => return,
        }
    }
}

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

#[cfg(all(test, unix))]
#[path = "pairing_tests.rs"]
mod tests;
