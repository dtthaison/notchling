//! The app's follower of Milestone 6's app-tier topics: `settings` (the switches), `accounts`,
//! `providers`, `limits` and `vault`. One connection of its own, like the pairing feed
//! (`phone::pairing`): it subscribes, reads `settings.switches`, then follows. It never turns a
//! switch on by itself: a switch the user chose on that a (restarted) daemon has off is only
//! offered back by the restart notice ([`super::restart`]); only [`SwitchFeed::choose`], from a
//! click in this app, sends `settings.set_switch`. It is the one owner of the three Milestone 6
//! switches' state in the app (the handover checklist reads it too). The views poll it: the
//! [`Switches`] every control is
//! drawn from, a counter per area that says "ask again" ([`Changes`]), the vault's pending
//! approvals and request forms (the daemon asks the person only while an app follows `vault`), and
//! the limit watchers' notices.
//!
//! A lost connection detaches the switches (every control goes off) and drops what was pending:
//! the daemon denied those uses when the app left. A switch turned off drops its area's pending
//! items too. A fixture root or a render follows nothing ([`SwitchFeed::none`]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notchling_local_api::{
    ops, topics, Client, ClientError, ClientKind, ConnectOptions, Event, SOCKET_NAME,
};
use serde_json::{json, Value};

use super::restart::{Area, RestartNotice, Said};
use super::{Switch, Switches};
use crate::actions::{Socket, Transport};
use crate::chat::data::DataRoot;
use crate::session_source::Backoff;

/// How many limit notices are kept, newest last (the daemon keeps 20 too).
const NOTICES_KEPT: usize = 20;

/// Where the user's choice per switch is kept.
pub trait SwitchChoice: Send + Sync {
    fn get(&self, switch: Switch) -> bool;
    fn set(&self, switch: Switch, on: bool) -> Result<(), String>;
}

/// No choice: off, and nowhere to keep one. A render's.
pub struct NoChoice;

impl SwitchChoice for NoChoice {
    fn get(&self, _: Switch) -> bool {
        false
    }
    fn set(&self, _: Switch, _: bool) -> Result<(), String> {
        Err("there is nowhere to keep the choice".into())
    }
}

/// The choices in the app's own settings file (`accountsOn`, `vaultOn`, `installOn`). Like
/// `actingOn`, they aren't in the engine's key list, so they are written past the key check.
pub struct SettingsChoice {
    pub file: std::path::PathBuf,
}

impl SwitchChoice for SettingsChoice {
    fn get(&self, switch: Switch) -> bool {
        crate::welcome::own::Flags {
            file: self.file.clone(),
        }
        .get(switch.choice_key())
    }

    fn set(&self, switch: Switch, on: bool) -> Result<(), String> {
        crate::welcome::own::Flags {
            file: self.file.clone(),
        }
        .set_choice(switch.choice_key(), on)
    }
}

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

/// Counts every event per area, so a view can tell something moved and ask the daemon again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub accounts: u64,
    pub providers: u64,
    pub limits: u64,
    pub vault: u64,
    /// `accounts.changed` events that said how a Move All ended (see [`SwitchFeed::last_move`]).
    pub moves: u64,
}

/// One thing a limit watcher said (`limits.notice {at, text}`).
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub at: f64,
    pub text: String,
}

#[derive(Default)]
struct State {
    switches: Switches,
    following: bool,
    changes: Changes,
    /// `vault.approval.pending` events, oldest first, each with its `id`.
    approvals: Vec<Value>,
    /// `vault.request.pending` events, oldest first.
    requests: Vec<Value>,
    notices: Vec<Notice>,
    /// The last `accounts.changed` that carried a Move All's end (`moved`, `errors`).
    last_move: Option<Value>,
}

struct Shared {
    state: Mutex<State>,
    stop: AtomicBool,
    restart: Arc<RestartNotice>,
}

/// Adds `data` to `list` unless one with its id is there.
fn push_pending(list: &mut Vec<Value>, data: &Value) {
    let id = data.get("id");
    if id.is_some_and(|id| id.is_string()) && !list.iter().any(|p| p.get("id") == id) {
        list.push(data.clone());
    }
}

fn remove_pending(list: &mut Vec<Value>, data: &Value) {
    let id = data.get("id");
    list.retain(|p| p.get("id") != id);
}

impl Shared {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The daemon's `vault.pending` reply, read on each (re)connect: what waits now replaces what
    /// was held (an id that's gone is dropped), each id once, oldest first. Events that follow
    /// still add and resolve by id. A reply that isn't the expected object changes nothing.
    fn load_pending(&self, reply: &Value) {
        let (Some(approvals), Some(requests)) = (
            reply.get("approvals").and_then(Value::as_array),
            reply.get("requests").and_then(Value::as_array),
        ) else {
            return;
        };
        let mut s = self.state();
        s.approvals.clear();
        s.requests.clear();
        for item in approvals {
            push_pending(&mut s.approvals, item);
        }
        for item in requests {
            push_pending(&mut s.requests, item);
        }
        s.changes.vault += 1;
    }

    fn apply(&self, event: &Event) {
        let mut s = self.state();
        match event.event.as_str() {
            topics::SWITCH_CHANGED => {
                let before = s.switches;
                if let Some(switch) = event
                    .data
                    .get("switch")
                    .and_then(Value::as_str)
                    .and_then(Switch::parse)
                {
                    let on = event.data.get("on").and_then(Value::as_bool) == Some(true);
                    if on {
                        self.restart.observe(Area::of(switch), Said::On, true);
                    } else {
                        // Turned off while the app follows: on purpose, not a restart.
                        self.restart.chose(Area::of(switch), false);
                    }
                }
                if s.switches.apply(&event.data) {
                    if before.is_on(Switch::Vault) && !s.switches.is_on(Switch::Vault) {
                        // Off denies every use still waiting and ends every form.
                        s.approvals.clear();
                        s.requests.clear();
                        s.changes.vault += 1;
                    }
                    if before.is_on(Switch::Accounts) != s.switches.is_on(Switch::Accounts) {
                        s.changes.accounts += 1;
                        s.changes.providers += 1;
                        s.changes.limits += 1;
                    }
                }
            }
            // An accounts op that outlasted its call answers here; Move All's carries what moved.
            topics::ACCOUNTS_DONE => {
                s.changes.accounts += 1;
                if event.data.get("op").and_then(Value::as_str) == Some(ops::ACCOUNTS_MOVE_ALL) {
                    let end = match event.data.get("result") {
                        Some(result) if event.data.get("ok") != Some(&Value::Bool(false)) => {
                            result.clone()
                        }
                        _ => {
                            let why = event.data.get("error").and_then(Value::as_str);
                            let why = why.unwrap_or("The move ended with an error.");
                            json!({"moved": [], "errors": {"accounts": why}})
                        }
                    };
                    s.changes.moves += 1;
                    s.last_move = Some(end);
                }
            }
            topics::ACCOUNTS_CHANGED | topics::ACCOUNTS_ADD_ENDED => {
                s.changes.accounts += 1;
                // A daemon that answers a Move All before it ends says how it went here.
                if event.event == topics::ACCOUNTS_CHANGED
                    && (event.data.get("moved").is_some() || event.data.get("errors").is_some())
                {
                    s.changes.moves += 1;
                    s.last_move = Some(event.data.clone());
                }
            }
            // The daemon publishes a "sync" change for every `providers.list`, the app's own
            // refresh included; counting those would make each refresh ask for the next.
            topics::PROVIDERS_CHANGED
                if event.data.get("reason").and_then(Value::as_str) == Some("sync") => {}
            topics::PROVIDERS_CHANGED => s.changes.providers += 1,
            "limits.notice" => {
                s.changes.limits += 1;
                let at = event.data.get("at").and_then(Value::as_f64).unwrap_or(0.0);
                let text = event
                    .data
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                if !text.is_empty() {
                    s.notices.push(Notice { at, text });
                    let extra = s.notices.len().saturating_sub(NOTICES_KEPT);
                    s.notices.drain(..extra);
                }
            }
            topics::VAULT_APPROVAL_PENDING => push_pending(&mut s.approvals, &event.data),
            topics::VAULT_APPROVAL_RESOLVED => remove_pending(&mut s.approvals, &event.data),
            topics::VAULT_REQUEST_PENDING => push_pending(&mut s.requests, &event.data),
            topics::VAULT_REQUEST_RESOLVED => remove_pending(&mut s.requests, &event.data),
            topics::VAULT_CHANGED => s.changes.vault += 1,
            _ => {}
        }
    }

    /// The connection is gone: nothing the daemon said holds.
    fn lost(&self) {
        let mut s = self.state();
        s.following = false;
        s.switches = Switches::detached();
        s.approvals.clear();
        s.requests.clear();
        s.changes.accounts += 1;
        s.changes.providers += 1;
        s.changes.limits += 1;
        s.changes.vault += 1;
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

/// The app's follower of the Milestone 6 topics. Dropping it ends the thread.
pub struct SwitchFeed {
    shared: Arc<Shared>,
    transport: Option<Arc<dyn Transport>>,
    choice: Arc<dyn SwitchChoice>,
}

impl SwitchFeed {
    /// Follows nothing, shows `switches`: a render's, a fixture root's.
    pub fn fixed(switches: Switches) -> SwitchFeed {
        SwitchFeed {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    switches,
                    ..State::default()
                }),
                stop: AtomicBool::new(false),
                restart: Arc::default(),
            }),
            transport: None,
            choice: Arc::new(NoChoice),
        }
    }

    /// Follows nothing, detached.
    pub fn none() -> SwitchFeed {
        SwitchFeed::fixed(Switches::detached())
    }

    /// The running app's: the daemon's socket, or nothing for a fixture root. What it learns about
    /// a restart goes to `restart`.
    pub fn for_run(root: Option<&DataRoot>, restart: Arc<RestartNotice>) -> SwitchFeed {
        match root.and_then(config_for) {
            Some(config) => {
                let transport = Socket {
                    socket: config.socket.clone(),
                    version: config.version.clone(),
                };
                let choice = SettingsChoice {
                    file: config.socket.with_file_name("settings.json"),
                };
                SwitchFeed::start(config, Arc::new(transport), Arc::new(choice), restart)
            }
            None => SwitchFeed::none(),
        }
    }

    /// Starts following; the user's choices come from `choice` and calls go through `transport`.
    pub fn start(
        config: FeedConfig,
        transport: Arc<dyn Transport>,
        choice: Arc<dyn SwitchChoice>,
        restart: Arc<RestartNotice>,
    ) -> SwitchFeed {
        let mut feed = SwitchFeed::none();
        feed.transport = Some(transport);
        feed.choice = choice.clone();
        if let Some(shared) = Arc::get_mut(&mut feed.shared) {
            shared.restart = restart;
        }
        let shared = feed.shared.clone();
        let spawned = std::thread::Builder::new()
            .name("notchling-switches".into())
            .spawn(move || run(&config, &shared, choice.as_ref()));
        if let Err(e) = spawned {
            eprintln!("follow the daemon's switches: {e}");
        }
        feed
    }

    /// Whether there is a daemon to call at all.
    pub fn is_live(&self) -> bool {
        self.transport.is_some()
    }

    /// Whether a connection follows the topics now.
    pub fn following(&self) -> bool {
        self.shared.state().following
    }

    pub fn switches(&self) -> Switches {
        self.shared.state().switches
    }

    pub fn changes(&self) -> Changes {
        self.shared.state().changes
    }

    /// The `vault.approval.pending` events still waiting, oldest first.
    pub fn approvals(&self) -> Vec<Value> {
        self.shared.state().approvals.clone()
    }

    /// The `vault.request.pending` events still waiting, oldest first.
    pub fn requests(&self) -> Vec<Value> {
        self.shared.state().requests.clone()
    }

    /// Takes a pending item off the list (answered, or closed).
    pub fn resolve(&self, id: &str) {
        let mut s = self.shared.state();
        let data = json!({"id": id});
        remove_pending(&mut s.approvals, &data);
        remove_pending(&mut s.requests, &data);
    }

    /// How the last Move All ended, when the daemon said so in an event rather than its reply.
    pub fn last_move(&self) -> Option<Value> {
        self.shared.state().last_move.clone()
    }

    /// What the limit watchers said, oldest first.
    pub fn notices(&self) -> Vec<Notice> {
        self.shared.state().notices.clone()
    }

    /// Whether the user chose `switch` on.
    pub fn chosen(&self, switch: Switch) -> bool {
        self.choice.get(switch)
    }

    /// The user chose, with a click in this app: tells the daemon (`settings.set_switch`), then
    /// keeps the choice. Blocking. The daemon goes first, so "off" always reaches it even when the
    /// settings file can't be written. The reply is applied at once; the event that follows says
    /// the same.
    pub fn choose(&self, switch: Switch, on: bool) -> Result<Switches, String> {
        let Some(transport) = &self.transport else {
            return Err(crate::acting::DETACHED_MESSAGE.into());
        };
        let (op, args) = switch.set_request(on);
        match transport.call(op, args) {
            Ok(reply) => {
                self.shared.restart.chose(Area::of(switch), on);
                if let Err(e) = self.choice.set(switch, on) {
                    eprintln!("{}: the choice isn't kept ({e})", switch.wire());
                }
                let mut s = self.shared.state();
                s.switches.apply(&reply);
                Ok(s.switches)
            }
            Err(e) => {
                if super::detaches(&e) {
                    self.shared.lost();
                }
                Err(match e {
                    ClientError::Api(api) => api.message,
                    other => other.to_string(),
                })
            }
        }
    }
}

impl Drop for SwitchFeed {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
    }
}

/// The topics followed; a daemon or tier without one of them is still followed for the rest.
const TOPICS: [&str; 5] = [
    topics::SETTINGS,
    topics::ACCOUNTS,
    topics::PROVIDERS,
    "limits",
    topics::VAULT,
];

fn run(config: &FeedConfig, shared: &Shared, choice: &dyn SwitchChoice) {
    let mut failures = 0u32;
    while !shared.stopped() {
        match connect(config) {
            Ok((client, switches, pending)) => {
                failures = 0;
                if let Some(pending) = pending {
                    shared.load_pending(&pending);
                }
                for switch in Switch::ALL {
                    let said = Said::from_bool(switches.is_on(switch));
                    shared
                        .restart
                        .observe(Area::of(switch), said, choice.get(switch));
                }
                {
                    let mut s = shared.state();
                    s.following = true;
                    s.switches = switches;
                    s.changes.accounts += 1;
                    s.changes.providers += 1;
                    s.changes.limits += 1;
                    s.changes.vault += 1;
                }
                follow(client, shared);
            }
            Err(_) => failures = failures.saturating_add(1),
        }
        shared.lost();
        sleep(config.backoff.delay(failures.saturating_sub(1)), shared);
    }
}

/// Connects, subscribes, then reads the switches and the vault's waiting approvals and forms
/// (`vault.pending`) on the same connection, so the first events seen are not older than what is
/// read: an agent that asked while the app was away still gets its prompt. Nothing is turned on.
fn connect(config: &FeedConfig) -> Result<(Client, Switches, Option<Value>), ClientError> {
    let options = ConnectOptions {
        check_server: config.check_server,
        ..ConnectOptions::default()
    };
    let mut client =
        Client::connect_with(&config.socket, ClientKind::App, &config.version, &options)?;
    for topic in TOPICS {
        match client.subscribe(&[topic]) {
            // An older daemon, or not the app tier: the rest still counts.
            Ok(_) | Err(ClientError::Api(_)) => {}
            Err(e) => return Err(e),
        }
    }
    let switches = match client.call(ops::SETTINGS_SWITCHES, json!({})) {
        Ok(reply) => Switches::from_reply(&reply),
        Err(ClientError::Api(_)) => Switches::all_off(),
        Err(e) => return Err(e),
    };
    // An older daemon has no such op: the events alone, as before.
    let pending = match client.call(ops::VAULT_PENDING, json!({})) {
        Ok(reply) => Some(reply),
        Err(ClientError::Api(_)) => None,
        Err(e) => return Err(e),
    };
    Ok((client, switches, pending))
}

fn follow(mut client: Client, shared: &Shared) {
    while !shared.stopped() {
        match client.next_event(Duration::from_millis(250)) {
            Ok(Some(event)) => shared.apply(&event),
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

#[cfg(test)]
#[path = "feed_unit_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "feed_tests.rs"]
mod feed_tests;
