use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notchling_engine::catalog::{ModelInfo, Tool as CatalogTool};
use notchling_engine::sessions::Agent;
use notchling_local_api::{
    ensure_daemon, ops, topics, Client, ClientError, ClientKind, ConnectOptions, Event, Started,
    SOCKET_NAME,
};
use serde_json::Value;

use crate::chat::data::DataRoot;
use crate::tools::live::ToolLive;

pub type Starter = Arc<dyn Fn(&Path) -> Result<Started, ClientError> + Send + Sync>;

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
    pub fn delay(&self, attempt: u32) -> Duration {
        self.first
            .checked_mul(1u32.checked_shl(attempt).unwrap_or(u32::MAX))
            .unwrap_or(self.max)
            .min(self.max)
    }
}

#[derive(Clone)]
pub struct SourceConfig {
    pub socket: PathBuf,
    pub start: Option<Starter>,
    pub backoff: Backoff,
    pub version: String,
    pub check_server: bool,
}

pub fn notchling_binary(exe: &Path) -> Option<PathBuf> {
    exe.parent()
        .map(|dir| dir.join(format!("notchling{}", std::env::consts::EXE_SUFFIX)))
}

pub fn config_for(root: &DataRoot, exe: Option<&Path>) -> Option<SourceConfig> {
    if root.fixture.is_some() {
        return None;
    }
    let start = exe.and_then(notchling_binary).map(|binary| {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    Files,
    Connecting,
    Daemon,
    Down,
    Untrusted,
}

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub agents: Option<Vec<Agent>>,
    pub prompts: Option<Vec<Value>>,
    pub tools: ToolLive,
    pub answered: HashSet<String>,
    pub link: Link,
    pub generation: u64,
}

impl View {
    pub fn daemon_down(&self) -> bool {
        self.link == Link::Down
    }

    pub fn note(&self) -> Option<&'static str> {
        match self.link {
            Link::Down => Some(DAEMON_DOWN_NOTE),
            Link::Untrusted => Some(DAEMON_UNTRUSTED_NOTE),
            Link::Files | Link::Connecting | Link::Daemon => None,
        }
    }
}

pub const DAEMON_DOWN_NOTE: &str = "Daemon not running";

pub const DAEMON_UNTRUSTED_NOTE: &str = "Daemon not trusted";

struct State {
    agents: Option<Vec<Agent>>,
    prompts: Option<Vec<Value>>,
    tools: ToolLive,
    catalog: HashMap<CatalogTool, Vec<ModelInfo>>,
    catalog_generation: u64,
    answered: HashSet<String>,
    link: Link,
    generation: u64,
}

struct Shared {
    state: Mutex<State>,
    stop: AtomicBool,
    catalog_ask: AtomicBool,
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

    fn set_catalog(&self, tool: CatalogTool, models: Vec<ModelInfo>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.catalog.get(&tool) != Some(&models) {
            state.catalog.insert(tool, models);
            state.catalog_generation += 1;
        }
    }

    fn set_prompts(&self, prompts: Option<Vec<Value>>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.prompts != prompts {
            state.prompts = prompts;
            state.generation += 1;
        }
    }

    fn apply(&self, event: Event) {
        match event.event.as_str() {
            topics::SESSIONS_CHANGED => {
                if let Ok(agents) = serde_json::from_value::<Vec<Agent>>(event.data) {
                    self.set(Link::Daemon, Some(agents));
                }
            }
            topics::PROMPT_PENDING | topics::PROMPT_RESOLVED => {
                let mut prompts = self
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .prompts
                    .clone();
                if let Some(list) = &mut prompts {
                    held_change(list, &event);
                    self.set_prompts(prompts);
                }
            }
            topics::CODEX_TURN | topics::SESSION_CHANGED => {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if state.tools.apply(&event) {
                    state.generation += 1;
                }
            }
            _ => {}
        }
    }

    fn reset_tools(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.tools != ToolLive::default() {
            state.tools = ToolLive::default();
            state.generation += 1;
        }
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

pub struct SessionSource {
    shared: Arc<Shared>,
}

impl SessionSource {
    pub fn files() -> SessionSource {
        SessionSource::with_link(Link::Files)
    }

    fn with_link(link: Link) -> SessionSource {
        SessionSource {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    agents: None,
                    prompts: None,
                    tools: ToolLive::default(),
                    catalog: HashMap::new(),
                    catalog_generation: 0,
                    answered: HashSet::new(),
                    link,
                    generation: 0,
                }),
                stop: AtomicBool::new(false),
                catalog_ask: AtomicBool::new(false),
            }),
        }
    }

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

    pub fn for_run(root: Option<&DataRoot>, exe: Option<&Path>) -> SessionSource {
        match root.and_then(|root| config_for(root, exe)) {
            Some(config) => SessionSource::start(config),
            None => SessionSource::files(),
        }
    }

    pub fn view(&self) -> View {
        let state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        View {
            agents: state.agents.clone(),
            prompts: state.prompts.clone(),
            tools: state.tools.clone(),
            answered: state.answered.clone(),
            link: state.link,
            generation: state.generation,
        }
    }

    pub fn catalog(&self, tool: CatalogTool) -> Vec<ModelInfo> {
        let state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.catalog.get(&tool).cloned().unwrap_or_default()
    }

    pub fn catalog_generation(&self) -> u64 {
        let state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.catalog_generation
    }

    pub fn ask_catalog(&self) {
        self.shared.catalog_ask.store(true, Ordering::SeqCst);
    }

    pub fn held(&self) -> (Option<Vec<Value>>, HashSet<String>) {
        let state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.prompts.clone(), state.answered.clone())
    }

    pub fn mark_answered(&self, key: &str) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.answered.insert(key.to_owned()) {
            state.generation += 1;
        }
    }

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

fn run(config: SourceConfig, shared: &Shared) {
    let mut tried_start = false;
    let mut failures = 0u32;
    while !shared.stopped() {
        let mut link = Link::Down;
        match connect(&config, &mut tried_start) {
            Ok((client, agents, prompts)) => {
                shared.set_prompts(prompts);
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
        shared.set_prompts(None);
        shared.reset_tools();
        shared.set(link, None);
        sleep(config.backoff.delay(failures.saturating_sub(1)), shared);
    }
}

type Connected = (Client, Vec<Agent>, Option<Vec<Value>>);

fn connect(config: &SourceConfig, tried_start: &mut bool) -> Result<Connected, ClientError> {
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
    let followed = match client.subscribe(&[topics::PROMPTS]) {
        Ok(_) => true,
        Err(ClientError::Api(_)) => false,
        Err(e) => return Err(e),
    };
    match client.subscribe(&[topics::TOOLS]) {
        Ok(_) | Err(ClientError::Api(_)) => {}
        Err(e) => return Err(e),
    }
    let mut agents = client.sessions::<Agent>()?;
    let mut prompts = match followed {
        true => match client.call(ops::PROMPTS_LIST, serde_json::json!({})) {
            Ok(Value::Array(list)) => Some(list),
            Ok(_) | Err(ClientError::Api(_)) => None,
            Err(e) => return Err(e),
        },
        false => None,
    };
    while let Some(event) = client.next_event(Duration::ZERO)? {
        match event.event.as_str() {
            topics::SESSIONS_CHANGED => {
                if let Ok(newer) = serde_json::from_value(event.data) {
                    agents = newer;
                }
            }
            _ => {
                if let Some(list) = &mut prompts {
                    held_change(list, &event);
                }
            }
        }
    }
    Ok((client, agents, prompts))
}

fn held_change(list: &mut Vec<Value>, event: &Event) {
    let id = event.data.get("id").cloned().unwrap_or(Value::Null);
    match event.event.as_str() {
        topics::PROMPT_PENDING => {
            if !list.iter().any(|p| p.get("id") == Some(&id)) {
                list.push(event.data.clone());
            }
        }
        topics::PROMPT_RESOLVED => list.retain(|p| p.get("id") != Some(&id)),
        _ => {}
    }
}

const CATALOG_EVERY: Duration = Duration::from_secs(3600);

fn catalog_due(last: Instant, now: Instant, asked: bool) -> bool {
    asked || now.saturating_duration_since(last) >= CATALOG_EVERY
}

fn follow(mut client: Client, shared: &Shared) {
    let mut last_read = Instant::now();
    if read_catalog(&mut client, shared).is_err() {
        return;
    }
    while !shared.stopped() {
        match client.next_event(Duration::from_millis(250)) {
            Ok(Some(event)) => {
                let changed = event.event == topics::CATALOG_CHANGED
                    && event.data.get("tool").and_then(Value::as_str)
                        == Some(CatalogTool::Claude.name());
                shared.apply(event);
                if changed {
                    last_read = Instant::now();
                    if read_catalog(&mut client, shared).is_err() {
                        return;
                    }
                }
            }
            Ok(None) => {}
            Err(_) => return,
        }
        let asked = shared.catalog_ask.swap(false, Ordering::SeqCst);
        if catalog_due(last_read, Instant::now(), asked) {
            last_read = Instant::now();
            if read_catalog(&mut client, shared).is_err() {
                return;
            }
        }
    }
}

fn read_catalog(client: &mut Client, shared: &Shared) -> Result<(), ClientError> {
    match client.call(
        ops::CATALOG_GET,
        serde_json::json!({ "tool": CatalogTool::Claude.name() }),
    ) {
        Ok(reply) => {
            if let Some(models) = parse_catalog(&reply) {
                shared.set_catalog(CatalogTool::Claude, models);
            }
            Ok(())
        }
        Err(ClientError::Api(_) | ClientError::Protocol(_) | ClientError::Timeout) => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn parse_catalog(reply: &Value) -> Option<Vec<ModelInfo>> {
    let models: Vec<ModelInfo> = serde_json::from_value(reply.get("models")?.clone()).ok()?;
    (!models.is_empty()).then_some(models)
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
#[path = "session_source_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "session_source_prompts_tests.rs"]
mod prompts_tests;
