use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notchling_local_api::{ops, topics, Client, ClientError, ClientKind, ConnectOptions, Event};
use serde_json::{json, Value};

use crate::chat::data::DataRoot;
use crate::switches::feed::{config_for, FeedConfig};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EmbedState {
    #[default]
    Off,
    NoModel,
    Warming,
    Indexing,
    Ready,
    Paused,
}

impl EmbedState {
    pub fn parse(name: &str) -> Option<EmbedState> {
        Some(match name {
            "off" => EmbedState::Off,
            "no_model" => EmbedState::NoModel,
            "warming" => EmbedState::Warming,
            "indexing" => EmbedState::Indexing,
            "ready" => EmbedState::Ready,
            "paused" => EmbedState::Paused,
            _ => return None,
        })
    }

    pub fn asks(self) -> bool {
        matches!(self, EmbedState::Ready | EmbedState::Indexing)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Meaning {
    pub state: EmbedState,
    pub chunks: usize,
    pub embedded: usize,
}

fn count(v: Option<&Value>) -> usize {
    v.and_then(Value::as_u64)
        .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX))
}

fn totals(coverage: &Value) -> (usize, usize) {
    match coverage {
        Value::Array(corpora) => corpora.iter().fold((0, 0), |(chunks, done), corpus| {
            let held = corpus
                .get("embedded")
                .and_then(Value::as_object)
                .map(|models| models.values().map(|n| count(Some(n))).min().unwrap_or(0))
                .unwrap_or(0);
            (chunks + count(corpus.get("chunks")), done + held)
        }),
        Value::Object(_) => (
            count(coverage.get("chunks")),
            count(coverage.get("vectors")),
        ),
        _ => (0, 0),
    }
}

impl Meaning {
    pub fn from_value(v: &Value) -> Option<Meaning> {
        let state = EmbedState::parse(v.get("state")?.as_str()?)?;
        let (chunks, embedded) = v
            .get("coverage")
            .or_else(|| v.get("corpora"))
            .map_or((0, 0), totals);
        Some(Meaning {
            state,
            chunks,
            embedded: embedded.min(chunks),
        })
    }

    pub fn asks(&self) -> bool {
        self.state.asks()
    }

    pub fn progress_text(&self) -> Option<String> {
        (self.state == EmbedState::Indexing && self.chunks > 0)
            .then(|| notchling_locale::search::meaning_progress(self.embedded, self.chunks))
    }
}

pub fn merge_changed(status: &mut Value, data: &Value) {
    let Some(map) = status.as_object_mut() else {
        return;
    };
    if let Some(state) = data.get("state") {
        map.insert("state".to_owned(), state.clone());
    }
    if let Some(coverage) = data.get("coverage") {
        map.insert("corpora".to_owned(), coverage.clone());
    }
    let Some(fetch) = data.get("fetch").filter(|f| f.is_object()) else {
        return;
    };
    let Some(model) = fetch.get("model").and_then(Value::as_str) else {
        return;
    };
    let state = fetch.get("state").and_then(Value::as_str).unwrap_or("");
    let mut rows: Vec<Value> = map
        .get("fetch")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    rows.retain(|r| r.get("model").and_then(Value::as_str) != Some(model));
    if matches!(state, "downloading" | "failed") {
        rows.push(fetch.clone());
    }
    map.insert("fetch".to_owned(), Value::Array(rows));
    let mut models: Vec<Value> = map
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    models.retain(|m| m.get("id").and_then(Value::as_str) != Some(model));
    if state == "ready" {
        models.push(json!({"id": model, "present": true, "verified": false}));
    }
    map.insert("models".to_owned(), Value::Array(models));
}

struct Shared {
    meaning: Mutex<(Meaning, bool)>,
    status: Mutex<Value>,
    stop: AtomicBool,
}

impl Shared {
    fn held(&self) -> std::sync::MutexGuard<'_, (Meaning, bool)> {
        self.meaning.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn status(&self) -> std::sync::MutexGuard<'_, Value> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn apply(&self, event: &Event) {
        if event.event == topics::EMBED_CHANGED {
            merge_changed(&mut self.status(), &event.data);
            if let Some(meaning) = Meaning::from_value(&event.data) {
                self.held().0 = meaning;
            }
        }
    }

    fn lost(&self) {
        *self.held() = (Meaning::default(), false);
        *self.status() = Value::Null;
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

pub struct EmbedFeed {
    shared: Arc<Shared>,
}

impl EmbedFeed {
    pub fn none() -> EmbedFeed {
        EmbedFeed {
            shared: Arc::new(Shared {
                meaning: Mutex::new((Meaning::default(), false)),
                status: Mutex::new(Value::Null),
                stop: AtomicBool::new(false),
            }),
        }
    }

    pub fn fixed(meaning: Meaning) -> EmbedFeed {
        let feed = EmbedFeed::none();
        *feed.shared.held() = (meaning, true);
        feed
    }

    pub fn with_status(self, status: Value) -> EmbedFeed {
        *self.shared.status() = status;
        self
    }

    pub fn status(&self) -> Value {
        self.shared.status().clone()
    }

    pub fn for_run(root: Option<&DataRoot>) -> EmbedFeed {
        match root.and_then(config_for) {
            Some(config) => EmbedFeed::start(config),
            None => EmbedFeed::none(),
        }
    }

    pub fn start(config: FeedConfig) -> EmbedFeed {
        let feed = EmbedFeed::none();
        let shared = feed.shared.clone();
        std::thread::Builder::new()
            .name("notchling-embed-feed".into())
            .spawn(move || run(&config, &shared))
            .ok();
        feed
    }

    pub fn meaning(&self) -> Meaning {
        self.shared.held().0
    }

    pub fn following(&self) -> bool {
        self.shared.held().1
    }
}

impl Drop for EmbedFeed {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
    }
}

type Seed = (Option<Meaning>, Value);

fn connect(config: &FeedConfig) -> Result<(Client, Seed), ClientError> {
    let options = ConnectOptions {
        check_server: config.check_server,
        ..ConnectOptions::default()
    };
    let mut client =
        Client::connect_with(&config.socket, ClientKind::App, &config.version, &options)?;
    match client.subscribe(&[topics::EMBED]) {
        Ok(_) | Err(ClientError::Api(_)) => {}
        Err(e) => return Err(e),
    }
    let seed = match client.call(ops::EMBED_STATUS, json!({})) {
        Ok(reply) => (Meaning::from_value(&reply), reply),
        Err(ClientError::Api(_)) => (None, Value::Null),
        Err(e) => return Err(e),
    };
    Ok((client, seed))
}

fn run(config: &FeedConfig, shared: &Shared) {
    let mut failures = 0u32;
    while !shared.stopped() {
        match connect(config) {
            Ok((client, (meaning, status))) => {
                failures = 0;
                *shared.status() = status;
                *shared.held() = (meaning.unwrap_or_default(), true);
                follow(client, shared);
            }
            Err(_) => failures = failures.saturating_add(1),
        }
        shared.lost();
        sleep(config.backoff.delay(failures.saturating_sub(1)), shared);
    }
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
#[path = "embed_feed_tests.rs"]
mod tests;
