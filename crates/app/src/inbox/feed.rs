use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use notchling_engine::inbox::ActionId;
use notchling_local_api::{ops, topics, Client, ClientError, ClientKind, ConnectOptions, Event};
use notchling_platform::notify::{system_notifier, Notifier, Unsupported};
use serde_json::{json, Value};

use super::model::{answer_args, seen_args, InboxModel};
use super::system::{notify_new, remember, Detached};
use crate::actions::{Socket, Transport};
use crate::chat::data::DataRoot;
use crate::switches::feed::{config_for, FeedConfig};

struct Shared {
    model: Mutex<InboxModel>,
    notified: Mutex<HashSet<String>>,
    notifier: Arc<dyn Notifier>,
    generation: AtomicU64,
    stop: AtomicBool,
}

impl Shared {
    fn new(notifier: Arc<dyn Notifier>) -> Shared {
        Shared {
            model: Mutex::default(),
            notified: Mutex::default(),
            notifier,
            generation: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        }
    }

    fn model(&self) -> MutexGuard<'_, InboxModel> {
        self.model.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn bump(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    fn notified(&self) -> MutexGuard<'_, HashSet<String>> {
        self.notified.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn seed(&self, reply: &Value) {
        let mut model = self.model();
        model.load(reply);
        remember(&model, &mut self.notified());
        drop(model);
        self.bump();
    }

    fn apply(&self, event: &Event) {
        let wire = json!({"event": event.event, "data": event.data});
        let (before, after) = {
            let mut model = self.model();
            let before = model.clone();
            model.apply(&wire);
            (before, model.clone())
        };
        if event.event == topics::INBOX_CHANGED {
            notify_new(&before, &after, &mut self.notified(), &*self.notifier);
        }
        self.bump();
    }

    fn lost(&self) {
        self.model().clear();
        self.bump();
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

pub struct InboxFeed {
    shared: Arc<Shared>,
    transport: Option<Arc<dyn Transport>>,
}

impl InboxFeed {
    pub fn none() -> InboxFeed {
        InboxFeed {
            shared: Arc::new(Shared::new(Arc::new(Unsupported))),
            transport: None,
        }
    }

    pub fn fixed(model: InboxModel) -> InboxFeed {
        let feed = InboxFeed::none();
        *feed.shared.model() = model;
        feed
    }

    pub fn for_run(root: Option<&DataRoot>) -> InboxFeed {
        match root.and_then(config_for) {
            Some(config) => {
                let transport = Socket {
                    socket: config.socket.clone(),
                    version: config.version.clone(),
                };
                let banners = Detached::new(Arc::from(system_notifier()));
                InboxFeed::start(config, Arc::new(transport), Arc::new(banners))
            }
            None => InboxFeed::none(),
        }
    }

    pub fn start(
        config: FeedConfig,
        transport: Arc<dyn Transport>,
        notifier: Arc<dyn Notifier>,
    ) -> InboxFeed {
        let mut feed = InboxFeed::none();
        feed.shared = Arc::new(Shared::new(notifier));
        feed.transport = Some(transport);
        let shared = feed.shared.clone();
        let spawned = std::thread::Builder::new()
            .name("notchling-inbox-feed".into())
            .spawn(move || run(&config, &shared));
        if let Err(e) = spawned {
            eprintln!("follow the daemon's inbox: {e}");
        }
        feed
    }

    pub fn is_live(&self) -> bool {
        self.transport.is_some()
    }

    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::SeqCst)
    }

    pub fn unread(&self) -> usize {
        self.shared.model().unread()
    }

    pub fn snapshot(&self, now_ms: i64) -> InboxModel {
        let mut model = self.shared.model().clone();
        model.set_now(now_ms);
        model
    }

    pub fn answer(&self, id: &str, action: ActionId) -> Result<bool, String> {
        let reply = self.call(ops::INBOX_ANSWER, answer_args(id, action))?;
        Ok(reply.get("answered").and_then(Value::as_bool) == Some(true))
    }

    pub fn seen(&self, ids: &[String]) -> Result<(), String> {
        if ids.is_empty() {
            return Ok(());
        }
        self.call(ops::INBOX_SEEN, seen_args(ids)).map(|_| ())
    }

    fn call(&self, op: &str, args: Value) -> Result<Value, String> {
        let Some(transport) = &self.transport else {
            return Err(crate::acting::DETACHED_MESSAGE.into());
        };
        transport.call(op, args).map_err(|e| match e {
            ClientError::Api(api) => api.message,
            other => other.to_string(),
        })
    }
}

impl Drop for InboxFeed {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
    }
}

fn run(config: &FeedConfig, shared: &Shared) {
    let mut failures = 0u32;
    while !shared.stopped() {
        match connect(config) {
            Ok((client, reply)) => {
                failures = 0;
                if let Some(reply) = reply {
                    shared.seed(&reply);
                }
                follow(client, shared);
            }
            Err(_) => failures = failures.saturating_add(1),
        }
        shared.lost();
        sleep(config.backoff.delay(failures.saturating_sub(1)), shared);
    }
}

fn connect(config: &FeedConfig) -> Result<(Client, Option<Value>), ClientError> {
    let options = ConnectOptions {
        check_server: config.check_server,
        ..ConnectOptions::default()
    };
    let mut client =
        Client::connect_with(&config.socket, ClientKind::App, &config.version, &options)?;
    match client.subscribe(&[topics::INBOX]) {
        Ok(_) | Err(ClientError::Api(_)) => {}
        Err(e) => return Err(e),
    }
    let reply = match client.call(ops::INBOX_LIST, json!({})) {
        Ok(reply) => Some(reply),
        Err(ClientError::Api(_)) => None,
        Err(e) => return Err(e),
    };
    Ok((client, reply))
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
#[path = "feed_tests.rs"]
mod tests;
