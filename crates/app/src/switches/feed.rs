//! The app's follower of Milestone 6's app-tier topics: `settings` (the switches), `accounts`,
//! `providers`, `limits` and `vault`. One connection of its own, like the pairing feed
//! (`phone::pairing`): it subscribes, reads `settings.switches`, asserts the user's choice for a
//! switch the daemon has off, then follows. The views poll it: the [`Switches`] every control is
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

use notchling_engine::settings::Settings;
use notchling_local_api::{
    ops, topics, Client, ClientError, ClientKind, ConnectOptions, Event, SOCKET_NAME,
};
use serde_json::{json, Value};

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
        Settings::open(&self.file)
            .ok()
            .and_then(|s| s.raw(switch.choice_key()).and_then(Value::as_bool))
            .unwrap_or(false)
    }

    fn set(&self, switch: Switch, on: bool) -> Result<(), String> {
        let mut store = Settings::open(&self.file).map_err(|e| e.to_string())?;
        let mut values = store.values().clone();
        values.insert(switch.choice_key().into(), Value::Bool(on));
        store.replace_all(values).map_err(|e| e.to_string())
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
}

struct Shared {
    state: Mutex<State>,
    stop: AtomicBool,
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

    fn apply(&self, event: &Event) {
        let mut s = self.state();
        match event.event.as_str() {
            topics::SWITCH_CHANGED => {
                let before = s.switches;
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
            topics::ACCOUNTS_CHANGED | topics::ACCOUNTS_ADD_ENDED => s.changes.accounts += 1,
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
            }),
            transport: None,
            choice: Arc::new(NoChoice),
        }
    }

    /// Follows nothing, detached.
    pub fn none() -> SwitchFeed {
        SwitchFeed::fixed(Switches::detached())
    }

    /// The running app's: the daemon's socket, or nothing for a fixture root.
    pub fn for_run(root: Option<&DataRoot>) -> SwitchFeed {
        match root.and_then(config_for) {
            Some(config) => {
                let transport = Socket {
                    socket: config.socket.clone(),
                    version: config.version.clone(),
                };
                let choice = SettingsChoice {
                    file: config.socket.with_file_name("settings.json"),
                };
                SwitchFeed::start(config, Arc::new(transport), Arc::new(choice))
            }
            None => SwitchFeed::none(),
        }
    }

    /// Starts following; the user's choices come from `choice` and calls go through `transport`.
    pub fn start(
        config: FeedConfig,
        transport: Arc<dyn Transport>,
        choice: Arc<dyn SwitchChoice>,
    ) -> SwitchFeed {
        let mut feed = SwitchFeed::none();
        feed.transport = Some(transport);
        feed.choice = choice.clone();
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

    /// What the limit watchers said, oldest first.
    pub fn notices(&self) -> Vec<Notice> {
        self.shared.state().notices.clone()
    }

    /// Whether the user chose `switch` on.
    pub fn chosen(&self, switch: Switch) -> bool {
        self.choice.get(switch)
    }

    /// The user chose: keeps it, then tells the daemon (`settings.set_switch`). Blocking. The
    /// reply is applied at once; the event that follows says the same.
    pub fn choose(&self, switch: Switch, on: bool) -> Result<Switches, String> {
        let Some(transport) = &self.transport else {
            return Err(crate::acting::DETACHED_MESSAGE.into());
        };
        self.choice.set(switch, on)?;
        let (op, args) = switch.set_request(on);
        match transport.call(op, args) {
            Ok(reply) => {
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
        match connect(config, choice) {
            Ok((client, switches)) => {
                failures = 0;
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

/// Connects, subscribes, reads the switches and asserts the user's choices on the same
/// connection, so the first events seen are not older than what is read.
fn connect(
    config: &FeedConfig,
    choice: &dyn SwitchChoice,
) -> Result<(Client, Switches), ClientError> {
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
    let mut switches = match client.call(ops::SETTINGS_SWITCHES, json!({})) {
        Ok(reply) => Switches::from_reply(&reply),
        Err(ClientError::Api(_)) => Switches::all_off(),
        Err(e) => return Err(e),
    };
    // Every daemon run starts off; the app brings it back to what the user chose.
    for switch in Switch::ALL {
        if !switches.is_on(switch) && choice.get(switch) {
            let (op, args) = switch.set_request(true);
            match client.call(op, args) {
                Ok(reply) => {
                    switches.apply(&reply);
                }
                Err(ClientError::Api(_)) => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok((client, switches))
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
mod tests {
    use super::*;

    fn event(name: &str, data: Value) -> Event {
        Event {
            event: name.to_owned(),
            data,
        }
    }

    fn shared() -> Shared {
        Shared {
            state: Mutex::new(State {
                switches: Switches::all_on(),
                following: true,
                ..State::default()
            }),
            stop: AtomicBool::new(false),
        }
    }

    #[test]
    fn pending_vault_items_come_and_go_by_id_and_off_drops_them() {
        let s = shared();
        s.apply(&event(
            topics::VAULT_APPROVAL_PENDING,
            json!({"id": "a1", "names": ["github"]}),
        ));
        s.apply(&event(
            topics::VAULT_APPROVAL_PENDING,
            json!({"id": "a1", "names": ["github"]}),
        ));
        s.apply(&event(topics::VAULT_REQUEST_PENDING, json!({"id": "r1"})));
        s.apply(&event(
            topics::VAULT_REQUEST_PENDING,
            json!({"name": "no id"}),
        ));
        assert_eq!(s.state().approvals.len(), 1, "the same id once");
        assert_eq!(
            s.state().requests.len(),
            1,
            "an item without an id is nothing"
        );
        s.apply(&event(
            topics::VAULT_APPROVAL_RESOLVED,
            json!({"id": "a1", "decision": "once"}),
        ));
        assert!(s.state().approvals.is_empty());
        s.apply(&event(topics::VAULT_CHANGED, json!({"names": ["github"]})));
        assert_eq!(s.state().changes.vault, 1);
        // The vault switch going off ends the form still open.
        s.apply(&event(
            topics::SWITCH_CHANGED,
            json!({"switch": "vault", "on": false}),
        ));
        assert!(s.state().requests.is_empty());
        assert!(!s.state().switches.vault);
        assert_eq!(s.state().changes.vault, 2);
    }

    #[test]
    fn the_areas_count_their_events_and_the_accounts_switch_moves_all_three() {
        let s = shared();
        s.apply(&event(topics::ACCOUNTS_CHANGED, json!({})));
        s.apply(&event(topics::ACCOUNTS_ADD_ENDED, json!({})));
        s.apply(&event(
            topics::PROVIDERS_CHANGED,
            json!({"provider": "codex"}),
        ));
        s.apply(&event(
            "limits.notice",
            json!({"at": 1.0, "text": "me is at 91%"}),
        ));
        s.apply(&event("limits.notice", json!({"at": 2.0, "text": ""})));
        s.apply(&event("something.else", json!({})));
        let c = s.state().changes;
        assert_eq!((c.accounts, c.providers, c.limits, c.vault), (2, 1, 2, 0));
        assert_eq!(
            s.state().notices,
            vec![Notice {
                at: 1.0,
                text: "me is at 91%".into()
            }]
        );
        s.apply(&event(
            topics::SWITCH_CHANGED,
            json!({"switch": "accounts", "on": false}),
        ));
        let c = s.state().changes;
        assert_eq!((c.accounts, c.providers, c.limits), (3, 2, 3));
        // The same again changes nothing.
        s.apply(&event(
            topics::SWITCH_CHANGED,
            json!({"switch": "accounts", "on": false}),
        ));
        assert_eq!(s.state().changes.accounts, 3);
    }

    #[test]
    fn notices_are_capped_and_a_lost_daemon_detaches_everything() {
        let s = shared();
        for i in 0..25 {
            s.apply(&event(
                "limits.notice",
                json!({"at": i as f64, "text": format!("n{i}")}),
            ));
        }
        let notices = s.state().notices.clone();
        assert_eq!(notices.len(), NOTICES_KEPT);
        assert_eq!(notices[0].text, "n5");
        s.apply(&event(topics::VAULT_APPROVAL_PENDING, json!({"id": "a1"})));
        s.lost();
        let st = s.state();
        assert!(!st.following && !st.switches.attached);
        assert!(st.approvals.is_empty());
    }

    #[test]
    fn a_feed_with_no_daemon_shows_its_fixed_switches_and_cannot_choose() {
        let feed = SwitchFeed::fixed(Switches::all_on());
        assert!(!feed.is_live() && !feed.following());
        assert!(feed.switches().is_on(Switch::Vault));
        assert!(feed.approvals().is_empty() && feed.requests().is_empty());
        assert!(!feed.chosen(Switch::Vault));
        assert!(feed.choose(Switch::Vault, true).is_err());
        let none = SwitchFeed::none();
        assert_eq!(none.switches(), Switches::detached());
    }

    #[test]
    fn the_choice_is_kept_in_the_apps_own_settings() {
        let scratch = crate::testutil::Scratch::new("switch-choice");
        let file = scratch.path().join("settings.json");
        let choice = SettingsChoice { file: file.clone() };
        assert!(!choice.get(Switch::Vault));
        choice.set(Switch::Vault, true).unwrap();
        assert!(SettingsChoice { file: file.clone() }.get(Switch::Vault));
        assert!(!choice.get(Switch::Accounts));
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("vaultOn"), "{text}");
    }
}

#[cfg(all(test, unix))]
#[path = "feed_tests.rs"]
mod feed_tests;
