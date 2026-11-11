//! The model catalog in the daemon: one [`CatalogService`] owns the engine's `Catalog` (the data
//! file with the user's `catalog.json` laid over it) and the live lists, and serves `catalog.get`.
//! Agent tier, read only, not switched: it reads Codex's cache file and the daemon's own cache,
//! and the one thing it writes is that cache.
//!
//! | Op | Tier | Args | Reply |
//! | --- | --- | --- | --- |
//! | `catalog.get` | agent | `{tool: "claude" \| "codex" \| "antigravity"}` | see below |
//!
//! ```json
//! {"tool": "claude", "fetchedAt": 1794150245.0,
//!  "models": [{"id": "claude-opus-5-5", "name": "Opus 5.5", "tier": "current",
//!              "contextWindow": 400000, "efforts": ["low", "medium", "high"],
//!              "defaultEffort": null, "price": {"input": 5.0, "output": 25.0, "cacheRead": 0.5},
//!              "borrowed": false}]}
//! ```
//!
//! - An unknown or missing `tool` is `bad_request`.
//! - Where each list comes from (the spec's table): Claude's is Anthropic's `GET /v1/models`, read
//!   with the main login's token and cached for a day, else the data file's; Codex's is the
//!   `models_cache.json` Codex keeps (re-read when its mtime changes); Antigravity's is the Model
//!   and Thinking Level options the last loaded `agy acp` session reported, cached. `fetchedAt` is
//!   when that list was fetched or seen (the cache file's mtime for Codex), `null` for the data
//!   file's list.
//! - A call for Claude when its list is due starts a fetch on a thread (one at a time) and answers
//!   from what it has; the fetch ends as `catalog.changed {tool}` on `tools`. It runs only while
//!   `accounts` is on, with the main login's access token read from Keychain by
//!   [`ClaudeAccounts::usage_token`], which never renews a login in use and answers nothing for an
//!   expired one. A failed fetch keeps the old list, and a try that found no token or failed is
//!   not repeated for [`RETRY_SECS`].

use crate::acting_ops::{now_ms, Room};
use crate::layout::Layout;
use crate::op_group::OpGroup;
use crate::switches::SwitchFlag;
use notchling_engine::accounts::claude::ClaudeAccounts;
use notchling_engine::accounts::http::HttpClient;
use notchling_engine::catalog::claude_live;
use notchling_engine::catalog::live_cache::{refresh_due, LiveCache};
use notchling_engine::catalog::{Catalog, LiveModel, ModelInfo, Tool};
use notchling_engine::tools::agy::ConfigOption;
use notchling_engine::tools::codex::models as codex_models;
use notchling_local_api::{ops, topics, ApiError, Caller, OpSpec};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// A try that found no token or failed is not repeated for this long, so a picker opened over
/// an expired login doesn't ask Keychain every time.
pub const RETRY_SECS: f64 = 900.0;

/// Runs a job off the caller's thread.
pub type Spawner = Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_secs() -> f64 {
    now_ms() as f64 / 1000.0
}

fn thread_spawner() -> Spawner {
    Arc::new(|job| {
        // A thread that can't start leaves the old list; the next call tries again.
        let _ = std::thread::Builder::new()
            .name("notchling-catalog".into())
            .spawn(job);
    })
}

/// Clears `fetching` however the fetch ends, also when its job is dropped without running (a
/// thread that couldn't start).
struct Fetching(Arc<Inner>);

impl Drop for Fetching {
    fn drop(&mut self) {
        self.0.fetching.store(false, Ordering::SeqCst);
    }
}

/// What the fetch thread needs, apart from the service so the thread can own it.
struct Inner {
    catalog: Arc<Catalog>,
    cache: LiveCache,
    claude: Option<(Arc<ClaudeAccounts>, Arc<SwitchFlag>)>,
    http: Arc<dyn HttpClient>,
    room: Option<Arc<dyn Room>>,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    /// A fetch is queued or running.
    fetching: AtomicBool,
    /// When the last fetch was tried, to hold off a retry.
    last_try: Mutex<Option<f64>>,
}

impl Inner {
    fn announce(&self, tool: Tool) {
        if let Some(room) = &self.room {
            room.publish(
                topics::TOOLS,
                topics::CATALOG_CHANGED,
                &json!({"tool": tool.name()}),
            );
        }
    }

    /// Reads the main login's token and fetches Anthropic's list. Anything missing or failing
    /// leaves the cache as it was.
    fn fetch_claude(&self, account: &str) {
        let Some((store, flag)) = &self.claude else {
            return;
        };
        if !flag.is_on() {
            return;
        }
        let Ok(Some(token)) = store.usage_token(account) else {
            return;
        };
        if let Ok(models) = claude_live::fetch(&*self.http, &token) {
            self.cache.put(Tool::Claude, (self.now)(), &models);
            self.announce(Tool::Claude);
        }
    }
}

pub struct CatalogService {
    inner: Arc<Inner>,
    codex_dir: PathBuf,
    /// Codex's list as read, and the mtime of the file it came from.
    codex: Mutex<Option<(SystemTime, Vec<LiveModel>)>>,
    spawner: Spawner,
}

impl CatalogService {
    /// `claude` is the accounts store and the `accounts` switch: without them (or while the
    /// switch is off) Claude's list is the cache or the data file's.
    pub fn new(
        catalog: Catalog,
        cache: LiveCache,
        codex_dir: PathBuf,
        claude: Option<(Arc<ClaudeAccounts>, Arc<SwitchFlag>)>,
        http: Arc<dyn HttpClient>,
    ) -> CatalogService {
        CatalogService {
            inner: Arc::new(Inner {
                catalog: Arc::new(catalog),
                cache,
                claude,
                http,
                room: None,
                now: Arc::new(now_secs),
                fetching: AtomicBool::new(false),
                last_try: Mutex::new(None),
            }),
            codex_dir,
            codex: Mutex::new(None),
            spawner: thread_spawner(),
        }
    }

    /// The daemon's: `<state dir>/catalog.json` over the data file, the live lists cached beside
    /// it in `live-models.json`, Codex's cache under its home, and the accounts store's own HTTP
    /// (which is gated on the same switch, and a refusal under a fixture root).
    pub fn for_daemon(
        layout: &Layout,
        claude: (Arc<ClaudeAccounts>, Arc<SwitchFlag>),
        room: Arc<dyn Room>,
    ) -> CatalogService {
        let state = layout.state_dir();
        let http = claude.0.env().http.clone();
        CatalogService::new(
            Catalog::load(&state.join("catalog.json")),
            LiveCache::new(state.join("live-models.json")),
            layout.codex_dir(),
            Some(claude),
            http,
        )
        .with_room(room)
    }

    /// Where `catalog.changed` goes.
    pub fn with_room(mut self, room: Arc<dyn Room>) -> CatalogService {
        self.inner_mut().room = Some(room);
        self
    }

    /// A fixed clock (tests).
    pub fn with_clock(mut self, now: Arc<dyn Fn() -> f64 + Send + Sync>) -> CatalogService {
        self.inner_mut().now = now;
        self
    }

    /// Runs the fetch with `spawner` instead of a thread (tests run it by hand).
    pub fn with_spawner(mut self, spawner: Spawner) -> CatalogService {
        self.spawner = spawner;
        self
    }

    /// Setup happens before the service is shared, so the `Arc` is still ours alone.
    fn inner_mut(&mut self) -> &mut Inner {
        Arc::get_mut(&mut self.inner).expect("the service is configured before it is shared")
    }

    /// The catalog every part of the daemon reads (cost prices, what a session may start on).
    pub fn catalog(&self) -> &Catalog {
        &self.inner.catalog
    }

    /// The same catalog, for a part that keeps its own handle (the cost tracker).
    pub fn shared_catalog(&self) -> Arc<Catalog> {
        self.inner.catalog.clone()
    }

    /// The tool's models: the live list laid over the data file as the catalog says.
    pub fn models(&self, tool: Tool) -> Vec<ModelInfo> {
        self.inner.catalog.models(tool, &self.live(tool))
    }

    /// Every model id a session of `tool` may start on.
    pub fn allowed(&self, tool: Tool) -> Vec<String> {
        self.inner.catalog.allowed(tool, &self.live(tool))
    }

    fn live(&self, tool: Tool) -> Vec<LiveModel> {
        match tool {
            Tool::Codex => self.codex_list().1,
            tool => self
                .inner
                .cache
                .get(tool)
                .map(|(_, models)| models)
                .unwrap_or_default(),
        }
    }

    /// When the tool's live list was fetched or seen; `None` while only the data file's is shown.
    fn fetched_at(&self, tool: Tool) -> Option<f64> {
        match tool {
            Tool::Codex => self.codex_list().0,
            tool => self.inner.cache.get(tool).map(|(at, _)| at),
        }
    }

    /// Codex's list, read again only when its file's mtime changed, and that mtime in seconds.
    fn codex_list(&self) -> (Option<f64>, Vec<LiveModel>) {
        let modified = std::fs::metadata(self.codex_dir.join("models_cache.json"))
            .and_then(|m| m.modified())
            .ok();
        let mut held = lock(&self.codex);
        let Some(modified) = modified else {
            *held = None;
            return (None, Vec::new());
        };
        if held.as_ref().is_none_or(|(at, _)| *at != modified) {
            let models = codex_models::load(&self.codex_dir)
                .into_iter()
                .map(|m| LiveModel {
                    id: m.slug,
                    name: Some(m.name),
                    context_window: m.context_window,
                    efforts: m.efforts,
                    default_effort: m.default_effort,
                })
                .collect();
            *held = Some((modified, models));
        }
        let secs = modified
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        (
            Some(secs),
            held.as_ref().map_or_else(Vec::new, |h| h.1.clone()),
        )
    }

    /// Starts a fetch of Anthropic's list on a thread when it is due: none already running, the
    /// `accounts` switch on, the main login known, the cached list over a day old (or none), and
    /// the last try not within [`RETRY_SECS`]. Otherwise it does nothing.
    pub fn refresh_claude(&self) {
        let inner = &self.inner;
        let Some((store, flag)) = &inner.claude else {
            return;
        };
        if !flag.is_on() {
            return;
        }
        let Some(account) = store.snapshot().active else {
            return;
        };
        let now = (inner.now)();
        if !refresh_due(inner.cache.get(Tool::Claude).map(|(at, _)| at), now) {
            return;
        }
        if inner
            .fetching
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        {
            let mut last = lock(&inner.last_try);
            if last.is_some_and(|at| now >= at && now - at < RETRY_SECS) {
                inner.fetching.store(false, Ordering::SeqCst);
                return;
            }
            *last = Some(now);
        }
        let fetching = Fetching(inner.clone());
        (self.spawner)(Box::new(move || {
            let fetching = fetching;
            fetching.0.fetch_claude(&account);
        }));
    }

    /// Records what an open `agy acp` session offers: the Model option's values as models, with
    /// the Thinking Level option's values as their efforts. Cached, and announced when the list
    /// differs from the one before. Options with no Model are not a list and change nothing.
    pub fn record_agy_options(&self, options: &[ConfigOption]) {
        let Some(model) = options.iter().find(|o| o.id == "model") else {
            return;
        };
        if model.values.is_empty() {
            return;
        }
        let level = options.iter().find(|o| o.id == "thought_level");
        let efforts: Vec<String> = level
            .map(|o| o.values.iter().map(|v| v.value.clone()).collect())
            .unwrap_or_default();
        let default_effort = level.map(|o| o.current.clone()).filter(|c| !c.is_empty());
        let models: Vec<LiveModel> = model
            .values
            .iter()
            .map(|v| LiveModel {
                id: v.value.clone(),
                name: Some(v.name.clone()),
                efforts: efforts.clone(),
                default_effort: default_effort.clone(),
                ..LiveModel::default()
            })
            .collect();
        let inner = &self.inner;
        let changed = inner
            .cache
            .get(Tool::Antigravity)
            .is_none_or(|(_, old)| old != models);
        inner.cache.put(Tool::Antigravity, (inner.now)(), &models);
        if changed {
            inner.announce(Tool::Antigravity);
        }
    }
}

impl OpGroup for CatalogService {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::CATALOG_GET).then_some(OpSpec::AGENT)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if op != ops::CATALOG_GET {
            return Err(ApiError::internal(format!("{op} has no handler")));
        }
        let tool = args
            .get("tool")
            .and_then(Value::as_str)
            .and_then(Tool::parse)
            .ok_or_else(|| {
                ApiError::bad_request("tool is \"claude\", \"codex\" or \"antigravity\"")
            })?;
        if tool == Tool::Claude {
            self.refresh_claude();
        }
        Ok(json!({
            "tool": tool.name(),
            "models": self.models(tool),
            "fetchedAt": self.fetched_at(tool),
        }))
    }

    fn topics(&self) -> &[&'static str] {
        &[topics::TOOLS]
    }
}
