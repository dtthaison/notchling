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

const NOTICES_KEPT: usize = 20;
const SETTINGS_KEPT: usize = 64;

pub trait SwitchChoice: Send + Sync {
    fn get(&self, switch: Switch) -> bool;
    fn set(&self, switch: Switch, on: bool) -> Result<(), String>;
}

pub struct NoChoice;

impl SwitchChoice for NoChoice {
    fn get(&self, _: Switch) -> bool {
        false
    }
    fn set(&self, _: Switch, _: bool) -> Result<(), String> {
        Err("there is nowhere to keep the choice".into())
    }
}

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

#[derive(Clone)]
pub struct FeedConfig {
    pub socket: std::path::PathBuf,
    pub version: String,
    pub check_server: bool,
    pub backoff: Backoff,
}

pub fn config_for(root: &DataRoot) -> Option<FeedConfig> {
    root.fixture.is_none().then(|| FeedConfig {
        socket: root.data_dir.join(SOCKET_NAME),
        version: env!("CARGO_PKG_VERSION").to_string(),
        check_server: true,
        backoff: Backoff::default(),
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub accounts: u64,
    pub providers: u64,
    pub limits: u64,
    pub vault: u64,
    pub moves: u64,
}

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
    approvals: Vec<Value>,
    requests: Vec<Value>,
    notices: Vec<Notice>,
    last_move: Option<Value>,
    settings: Vec<String>,
}

struct Shared {
    state: Mutex<State>,
    stop: AtomicBool,
    restart: Arc<RestartNotice>,
}

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
                        self.restart.chose(Area::of(switch), false);
                    }
                }
                if s.switches.apply(&event.data) {
                    if before.is_on(Switch::Vault) && !s.switches.is_on(Switch::Vault) {
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
                if event.event == topics::ACCOUNTS_CHANGED
                    && (event.data.get("moved").is_some() || event.data.get("errors").is_some())
                {
                    s.changes.moves += 1;
                    s.last_move = Some(event.data.clone());
                }
            }
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
            topics::SETTINGS_CHANGED => {
                let key = event.data.get("key").and_then(Value::as_str);
                if key.is_some_and(|k| crate::accounts::wire::Call::LIMIT_KEYS.contains(&k)) {
                    s.changes.limits += 1;
                }
                if let Some(key) = key.filter(|k| !s.settings.iter().any(|seen| seen == k)) {
                    s.settings.push(key.to_owned());
                    let extra = s.settings.len().saturating_sub(SETTINGS_KEPT);
                    s.settings.drain(..extra);
                }
            }
            _ => {}
        }
    }

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

pub struct SwitchFeed {
    shared: Arc<Shared>,
    transport: Option<Arc<dyn Transport>>,
    choice: Arc<dyn SwitchChoice>,
}

impl SwitchFeed {
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

    pub fn none() -> SwitchFeed {
        SwitchFeed::fixed(Switches::detached())
    }

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

    pub fn is_live(&self) -> bool {
        self.transport.is_some()
    }

    pub fn following(&self) -> bool {
        self.shared.state().following
    }

    pub fn switches(&self) -> Switches {
        self.shared.state().switches
    }

    pub fn changes(&self) -> Changes {
        self.shared.state().changes
    }

    pub fn approvals(&self) -> Vec<Value> {
        self.shared.state().approvals.clone()
    }

    pub fn requests(&self) -> Vec<Value> {
        self.shared.state().requests.clone()
    }

    pub fn resolve(&self, id: &str) {
        let mut s = self.shared.state();
        let data = json!({"id": id});
        remove_pending(&mut s.approvals, &data);
        remove_pending(&mut s.requests, &data);
    }

    pub fn last_move(&self) -> Option<Value> {
        self.shared.state().last_move.clone()
    }

    pub fn notices(&self) -> Vec<Notice> {
        self.shared.state().notices.clone()
    }

    pub fn take_settings(&self) -> Vec<String> {
        std::mem::take(&mut self.shared.state().settings)
    }

    pub fn chosen(&self, switch: Switch) -> bool {
        self.choice.get(switch)
    }

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

fn shown_keys() -> Vec<String> {
    notchling_engine::settings::KEYS
        .iter()
        .filter(|k| k.access.shown())
        .map(|k| k.name.to_owned())
        .collect()
}

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
                    s.settings = shown_keys();
                }
                follow(client, shared);
            }
            Err(_) => failures = failures.saturating_add(1),
        }
        shared.lost();
        sleep(config.backoff.delay(failures.saturating_sub(1)), shared);
    }
}

fn connect(config: &FeedConfig) -> Result<(Client, Switches, Option<Value>), ClientError> {
    let options = ConnectOptions {
        check_server: config.check_server,
        ..ConnectOptions::default()
    };
    let mut client =
        Client::connect_with(&config.socket, ClientKind::App, &config.version, &options)?;
    for topic in TOPICS {
        match client.subscribe(&[topic]) {
            Ok(_) | Err(ClientError::Api(_)) => {}
            Err(e) => return Err(e),
        }
    }
    let switches = match client.call(ops::SETTINGS_SWITCHES, json!({})) {
        Ok(reply) => Switches::from_reply(&reply),
        Err(ClientError::Api(_)) => Switches::all_off(),
        Err(e) => return Err(e),
    };
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
