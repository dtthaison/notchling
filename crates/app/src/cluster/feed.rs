use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use notchling_engine::vault::entry::VaultEntry;
use notchling_engine::versions::Tool;
use notchling_local_api::{ops, topics, Client, ClientError, ClientKind, ConnectOptions, Event};
use notchling_locale as t;
use serde_json::{json, Value};

use super::add_runs::AddRuns;
use super::add_source::AddSource;
use super::model::ClusterModel;
use super::tool_runs::ToolRuns;
use super::updates_state::UpdateSession;
use super::wire::{self, Request};
use crate::actions::{Socket, Transport};
use crate::chat::data::DataRoot;
use crate::machines::versions_feed::{mac_side, MacSide};
use crate::switches::feed::{config_for, FeedConfig};
use crate::vault::store::Vault;

const REFRESH_EVERY: Duration = Duration::from_secs(10);
const MOVE_REFRESH_EVERY: Duration = Duration::from_secs(1);

struct Shared {
    model: Mutex<ClusterModel>,
    last: Mutex<Value>,
    generation: Arc<AtomicU64>,
    stop: AtomicBool,
    adds: AddRuns,
    updates: Arc<UpdateSession>,
    runs: Arc<ToolRuns>,
    mac: Mutex<Option<MacSide>>,
    reported: Mutex<BTreeMap<(String, String), (Value, i64)>>,
}

pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn tool_prints(member: &Value) -> BTreeMap<String, Value> {
    let versions = member.get("versions").unwrap_or(&Value::Null);
    let kinds = ["tools", "logins", "own_logins", "ownLogins"];
    let mut keys: Vec<String> = Vec::new();
    for kind in kinds {
        if let Some(map) = versions.get(kind).and_then(Value::as_object) {
            keys.extend(map.keys().cloned());
        }
    }
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .map(|key| {
            let print = json!(kinds.map(|kind| versions.get(kind).and_then(|m| m.get(&key))));
            (key, print)
        })
        .collect()
}

impl Shared {
    fn new() -> Shared {
        let generation = Arc::new(AtomicU64::new(0));
        Shared {
            model: Mutex::default(),
            last: Mutex::new(Value::Null),
            adds: AddRuns::tied_to(generation.clone()),
            runs: Arc::new(ToolRuns::tied_to(generation.clone())),
            mac: Mutex::new(None),
            reported: Mutex::new(BTreeMap::new()),
            generation,
            stop: AtomicBool::new(false),
            updates: Arc::new(UpdateSession::default()),
        }
    }

    fn model(&self) -> MutexGuard<'_, ClusterModel> {
        self.model.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn bump(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    fn remember(&self, status: &Value) {
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = status.clone();
    }

    fn track(&self, status: &Value, now: i64) {
        let Some(members) = status.get("members").and_then(Value::as_array) else {
            return;
        };
        let mut reported = self.reported.lock().unwrap_or_else(|e| e.into_inner());
        for member in members {
            let Some(id) = member.get("id").and_then(Value::as_str) else {
                continue;
            };
            let mut prints = tool_prints(member);
            for (node, key) in reported.keys() {
                if node == id {
                    prints.entry(key.clone()).or_insert(Value::Null);
                }
            }
            for (key, print) in prints {
                match reported.get_mut(&(id.to_owned(), key.clone())) {
                    Some((old, at)) => {
                        if *old != print {
                            *old = print;
                            *at = now;
                        }
                    }
                    None => {
                        reported.insert((id.to_owned(), key), (print, 0));
                    }
                }
            }
        }
    }

    fn load(&self, status: &Value) {
        self.remember(status);
        self.track(status, now_ms());
        let me = status.get("me").and_then(Value::as_str).unwrap_or("");
        let next = ClusterModel::from_status(status, me);
        self.adds.seen(&next);
        let mut model = self.model();
        if *model != next {
            *model = next;
            drop(model);
            self.bump();
        }
    }

    fn apply(&self, event: &Event) {
        if event.event == topics::CLUSTER_CHANGED {
            let merged = {
                let last = self.last.lock().unwrap_or_else(|e| e.into_inner());
                carried(&last, &event.data)
            };
            self.load(&merged);
        }
    }

    fn set_mac(&self, side: Option<MacSide>) {
        let mut mac = self.mac.lock().unwrap_or_else(|e| e.into_inner());
        if *mac != side {
            *mac = side;
            drop(mac);
            self.bump();
        }
    }

    fn lost(&self) {
        self.set_mac(None);
        self.remember(&Value::Null);
        let mut model = self.model();
        if model.is_loaded() {
            *model = ClusterModel::default();
            drop(model);
            self.bump();
        }
    }

    fn refresh_every(&self) -> Duration {
        let model = self.model();
        if model.move_running() || model.promote_running() {
            MOVE_REFRESH_EVERY
        } else {
            REFRESH_EVERY
        }
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

const READ_ONLY_KEYS: [&str; 5] = ["replication", "move", "promote", "waiting", "jobs"];

pub(super) fn carried(last: &Value, event: &Value) -> Value {
    let mut next = event.clone();
    for key in READ_ONLY_KEYS {
        if next.get(key).is_none() {
            if let Some(old) = last.get(key) {
                next[key] = old.clone();
            }
        }
    }
    let old_members = last.get("members").and_then(Value::as_array);
    if let (Some(members), Some(old)) = (next["members"].as_array_mut(), old_members) {
        for member in members.iter_mut().filter(|m| m.get("ready").is_none()) {
            let ready = old
                .iter()
                .find(|o| o.get("id") == member.get("id"))
                .and_then(|o| o.get("ready"));
            if let Some(ready) = ready {
                member["ready"] = ready.clone();
            }
        }
    }
    next
}

pub struct ClusterFeed {
    shared: Arc<Shared>,
    transport: Option<Arc<dyn Transport>>,
    remote_vault: Arc<Vault>,
    add: Arc<AddSource>,
}

impl ClusterFeed {
    pub fn none() -> ClusterFeed {
        ClusterFeed {
            shared: Arc::new(Shared::new()),
            transport: None,
            remote_vault: Arc::new(Vault::remote_fixed(Vec::new())),
            add: Arc::new(AddSource::none()),
        }
    }

    pub fn fixed_adding(model: ClusterModel, add: AddSource, runs: AddRuns) -> ClusterFeed {
        let mut feed = ClusterFeed::fixed(model.clone());
        feed.add = Arc::new(add);
        if let Some(shared) = Arc::get_mut(&mut feed.shared) {
            runs.seen(&model);
            shared.adds = runs;
        }
        feed
    }

    pub fn fixed(model: ClusterModel) -> ClusterFeed {
        let feed = ClusterFeed::none();
        feed.shared.adds.seen(&model);
        *feed.shared.model() = model;
        feed
    }

    pub fn fixed_with_vault(model: ClusterModel, entries: Vec<VaultEntry>) -> ClusterFeed {
        let mut feed = ClusterFeed::fixed(model);
        feed.remote_vault = Arc::new(Vault::remote_fixed(entries));
        feed
    }

    pub fn remote_vault(&self) -> Arc<Vault> {
        self.remote_vault.clone()
    }

    pub fn for_run(root: Option<&DataRoot>) -> ClusterFeed {
        match root.and_then(config_for) {
            Some(config) => {
                let transport = Socket {
                    socket: config.socket.clone(),
                    version: config.version.clone(),
                };
                let mut feed = ClusterFeed::start(config, Arc::new(transport));
                if let Some(root) = root {
                    feed.add = Arc::new(AddSource::for_root(root.clone()));
                }
                feed
            }
            None => ClusterFeed::none(),
        }
    }

    pub fn start(config: FeedConfig, transport: Arc<dyn Transport>) -> ClusterFeed {
        let mut feed = ClusterFeed::none();
        let shared = feed.shared.clone();
        let spawned = std::thread::Builder::new()
            .name("notchling-cluster-feed".into())
            .spawn(move || run(&config, &shared));
        if spawned.is_ok() {
            feed.remote_vault = Arc::new(Vault::remote_over(transport.clone()));
            feed.transport = Some(transport);
        }
        feed
    }

    pub fn is_live(&self) -> bool {
        self.transport.is_some()
    }

    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::SeqCst)
    }

    pub fn adds(&self) -> AddRuns {
        self.shared.adds.clone()
    }

    pub fn updates(&self) -> Arc<UpdateSession> {
        self.shared.updates.clone()
    }

    pub fn tool_runs(&self) -> Arc<ToolRuns> {
        self.shared.runs.clone()
    }

    pub fn mac(&self) -> Option<MacSide> {
        self.shared
            .mac
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    #[cfg(test)]
    pub(super) fn take_status(&self, status: &Value) {
        self.shared.load(status);
    }

    pub fn reported_at(&self, node: &str, tool: Tool) -> Option<i64> {
        self.shared
            .reported
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(node.to_owned(), tool.name().to_owned()))
            .map(|(_, at)| *at)
    }

    pub fn with_mac(self, side: MacSide) -> ClusterFeed {
        self.shared.set_mac(Some(side));
        self
    }

    pub fn add_source(&self) -> Arc<AddSource> {
        self.add.clone()
    }

    pub fn snapshot(&self) -> ClusterModel {
        self.shared.model().clone()
    }

    pub fn send(&self, request: &Request) -> Result<Value, String> {
        let Some(transport) = &self.transport else {
            return Err(crate::acting::DETACHED_MESSAGE.into());
        };
        transport
            .call(request.op, request.args.clone())
            .map_err(|e| {
                let text = match e {
                    ClientError::Api(api) => api.message,
                    other => other.to_string(),
                };
                wire::without_token(&text, &request.args)
            })
    }

    pub fn token(&self) -> Result<String, String> {
        let reply = self.send(&wire::join_token())?;
        wire::token_of(&reply).ok_or_else(|| t::cluster::token_none().to_owned())
    }

    pub fn refresh(&self) -> bool {
        let read = match self.send(&wire::status()) {
            Ok(status) => {
                self.shared.load(&status);
                true
            }
            Err(_) => false,
        };
        if let Some(transport) = &self.transport {
            let side = transport
                .call(ops::VERSIONS_GET, json!({}))
                .ok()
                .and_then(|reply| mac_side(&reply).ok());
            self.shared.set_mac(side);
        }
        read
    }
}

impl Drop for ClusterFeed {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
    }
}

fn run(config: &FeedConfig, shared: &Shared) {
    let mut failures = 0u32;
    while !shared.stopped() {
        match connect(config) {
            Ok((client, status)) => {
                failures = 0;
                shared.load(&status);
                follow(client, shared);
            }
            Err(_) => failures = failures.saturating_add(1),
        }
        shared.lost();
        sleep(config.backoff.delay(failures.saturating_sub(1)), shared);
    }
}

fn connect(config: &FeedConfig) -> Result<(Client, Value), ClientError> {
    let options = ConnectOptions {
        check_server: config.check_server,
        ..ConnectOptions::default()
    };
    let mut client =
        Client::connect_with(&config.socket, ClientKind::App, &config.version, &options)?;
    match client.subscribe(&[topics::CLUSTER]) {
        Ok(()) | Err(ClientError::Api(_)) => {}
        Err(e) => return Err(e),
    }
    let request = wire::status();
    let status = client.call(request.op, request.args)?;
    Ok((client, status))
}

fn read_mac(client: &mut Client, shared: &Shared) {
    let side = client
        .call(ops::VERSIONS_GET, json!({}))
        .ok()
        .and_then(|reply| mac_side(&reply).ok());
    shared.set_mac(side);
}

fn follow(mut client: Client, shared: &Shared) {
    read_mac(&mut client, shared);
    let mut last = Instant::now();
    while !shared.stopped() {
        match client.next_event(Duration::from_millis(250)) {
            Ok(Some(event)) => shared.apply(&event),
            Ok(None) => {}
            Err(_) => return,
        }
        if last.elapsed() >= shared.refresh_every() {
            last = Instant::now();
            let request = wire::status();
            match client.call(request.op, request.args) {
                Ok(status) => shared.load(&status),
                Err(ClientError::Api(_)) => {}
                Err(_) => return,
            }
            read_mac(&mut client, shared);
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
