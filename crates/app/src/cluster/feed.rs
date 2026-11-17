use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use notchling_engine::vault::entry::VaultEntry;
use notchling_local_api::{topics, Client, ClientError, ClientKind, ConnectOptions, Event};
use notchling_locale as t;
use serde_json::Value;

use super::add_runs::AddRuns;
use super::add_source::AddSource;
use super::model::ClusterModel;
use super::wire::{self, Request};
use crate::actions::{Socket, Transport};
use crate::chat::data::DataRoot;
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
}

impl Shared {
    fn new() -> Shared {
        let generation = Arc::new(AtomicU64::new(0));
        Shared {
            model: Mutex::default(),
            last: Mutex::new(Value::Null),
            adds: AddRuns::tied_to(generation.clone()),
            generation,
            stop: AtomicBool::new(false),
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

    fn load(&self, status: &Value) {
        self.remember(status);
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

    fn lost(&self) {
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

const READ_ONLY_KEYS: [&str; 4] = ["replication", "move", "promote", "waiting"];

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
        let mut feed = ClusterFeed::fixed(model);
        feed.add = Arc::new(add);
        if let Some(shared) = Arc::get_mut(&mut feed.shared) {
            shared.adds = runs;
        }
        feed
    }

    pub fn fixed(model: ClusterModel) -> ClusterFeed {
        let feed = ClusterFeed::none();
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

    pub fn refresh(&self) {
        if let Ok(status) = self.send(&wire::status()) {
            self.shared.load(&status);
        }
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

fn follow(mut client: Client, shared: &Shared) {
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
