//! The model catalog in the daemon (decision 0016, phase 1): one [`CatalogService`] owns the
//! engine's `Catalog` (the data file with the user's `catalog.json` laid over it) and the live
//! lists, and serves `catalog.get`. Agent tier, read only, not switched: it reads Codex's cache
//! file and the daemon's own cache, and the one thing it writes is that cache.
//!
//! | Op | Tier | Args | Reply |
//! | --- | --- | --- | --- |
//! | `catalog.get` | agent | `{tool: "claude" \| "codex" \| "antigravity"}` | see below |
//!
//! | Event | Topic | Body | When |
//! | --- | --- | --- | --- |
//! | `catalog.changed` | `tools` | `{tool}` | a fetch of Claude's list lands, or a loaded `agy acp` session reports options that differ from the cached ones |
//!
//! ```json
//! {"tool": "claude", "fetchedAt": 1794150245.0,
//!  "models": [{"id": "claude-opus-5-5", "name": "Opus 5.5", "tier": "current",
//!              "contextWindow": 400000, "efforts": ["low", "medium", "high", "xhigh", "max"],
//!              "defaultEffort": null, "price": {"input": 4.0, "output": 20.0, "cacheRead": 0.2},
//!              "borrowed": false}]}
//! ```
//!
//! - A `borrowed` model also has `borrowedFrom`, the known model whose price and window it uses.
//! - An unknown or missing `tool` is `bad_request`.
//! - Where each list comes from (the spec's table): Claude's is Anthropic's `GET /v1/models`, read
//!   with the main login's token and cached for a day; while none has ever been read (no token,
//!   or every fetch failed), Claude Code's own `additionalModelOptionsCache` in its main
//!   `~/.claude.json` (read only, re-read when its mtime changes); else the data file's. Codex's
//!   is the `models_cache.json` Codex keeps (re-read when its mtime changes); Antigravity's is the
//!   Model and Thinking Level options the last loaded `agy acp` session reported, cached.
//!   `fetchedAt` is when that list was fetched or seen (the file's mtime for Codex's and Claude
//!   Code's), `null` for the data file's list.
//! - A call for Claude when its list is due (a day old, or none) starts a fetch on a thread (one at
//!   a time) and answers from what it has; the fetch ends as `catalog.changed {tool}` on `tools`.
//!   It runs only while `accounts` is on, with the main login's access token read from Keychain by
//!   [`ClaudeAccounts::live_token`], which only reads the live login, never renews anything and
//!   answers nothing for an expired one. A failed fetch keeps the old list, and a try that found
//!   no token or failed is not repeated for [`RETRY_SECS`]. After the endpoint refused a login
//!   (400, 403 or 404; a 401 or 429 passes with time) the same token is not sent again for
//!   [`REFUSED_RETRY_SECS`] (a day), told by its SHA-256 fingerprint, never kept itself; a new
//!   live token is sent at the next try. The first failure of a run is logged
//!   to standard error by its status alone, never its body or the token. Chat asks at start, every hour and when
//!   a Claude picker menu opens, so the list is fetched about once a day while Chat runs.
//! - The user's `catalog.json` (beside the socket) fixes prices, windows and names, entry by
//!   entry; it never adds a model to what a session may start on. It is read once, when the
//!   daemon starts, so an edit applies at the next start (and the cost cache then reprices the
//!   models whose price changed). Its shape is the data file's, per tool (`claude`, `codex`,
//!   `antigravity`): `prices` (a list of `{"match": "fable-5-3", "input": 9.0, "output": 45.0,
//!   "cache_read": 0.3}` in dollars per million tokens; a Claude key may be bare or the full
//!   `claude-` id, another tool's is its whole id), `windows` and `names` (objects keyed by the
//!   full model id). An entry that doesn't parse, or a negative price, is skipped alone.
//! - The same catalog is read elsewhere in the daemon: `cost.summary`'s prices (`cost_ops`), the
//!   `session.new --model` check (`control_ops`, through [`CatalogService::is_allowed`]: the
//!   built-in models plus the live lists, a Claude id in the shape `[a-z][a-z0-9.-]*` with an
//!   optional `[1m]`), and the phone's `models` list and `modelId` (`bridge_backend`). A `/model`
//!   sent through `session.command` isn't checked against it yet.

use crate::acting_ops::{now_ms, Room};
use crate::layout::Layout;
use crate::op_group::OpGroup;
use crate::switches::SwitchFlag;
use notchling_engine::accounts::claude::ClaudeAccounts;
use notchling_engine::accounts::http::{HttpClient, HttpRequest, HttpResponse};
use notchling_engine::catalog::live_cache::{refresh_due, LiveCache};
use notchling_engine::catalog::{claude_code, claude_live};
use notchling_engine::catalog::{model_key, Catalog, LiveModel, ModelInfo, Tool};
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

/// After the endpoint refused a login (400, 403, 404), that token is not sent again for a day:
/// it would be refused again. A new token is sent at the next try.
pub const REFUSED_RETRY_SECS: f64 = 86_400.0;

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
    /// When the last fetch was tried, to hold off a retry for [`RETRY_SECS`].
    last_try: Mutex<Option<f64>>,
    /// When the endpoint refused a login (400, 403, 404) and that token's fingerprint (never the
    /// token): the same token isn't sent again for [`REFUSED_RETRY_SECS`]; another one is.
    refused: Mutex<Option<(f64, [u8; 32])>>,
    /// Where a failed fetch is logged: the daemon's log, standard error.
    log: Box<dyn Fn(&str) + Send + Sync>,
    /// A failed fetch was logged this run; later ones aren't.
    logged: AtomicBool,
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

    /// Reads the main login's live token (read only: [`ClaudeAccounts::live_token`] has no saved
    /// slot, lane or renewal on its path, so whichever account is main when this runs, no refresh
    /// token is rotated) and fetches Anthropic's list. Anything missing or failing leaves the
    /// cache as it was. The first failure of a run is logged, by its status only. After a
    /// refusal (400, 403, 404) the same token is not sent again for [`REFUSED_RETRY_SECS`]; a
    /// new live token (another login, or the same one renewed by Claude Code) is.
    fn fetch_claude(&self) {
        let Some((store, flag)) = &self.claude else {
            return;
        };
        if !flag.is_on() {
            return;
        }
        let Ok(Some(token)) = store.live_token() else {
            return;
        };
        let fingerprint = claude_live::token_fingerprint(&token);
        let now = (self.now)();
        {
            let refused = lock(&self.refused);
            if refused.is_some_and(|(at, seen)| {
                seen == fingerprint && now >= at && now - at < REFUSED_RETRY_SECS
            }) {
                return;
            }
        }
        let answer = claude_live::fetch(&*self.http, &token);
        *lock(&self.refused) = match &answer {
            Err(why) if why.refused() => Some((now, fingerprint)),
            _ => None,
        };
        match answer {
            Ok(models) => {
                self.cache.put(Tool::Claude, (self.now)(), &models);
                self.announce(Tool::Claude);
            }
            Err(why) => {
                if !self.logged.swap(true, Ordering::SeqCst) {
                    (self.log)(&format!(
                        "notchling: Claude's model list wasn't read ({why}); the pickers show \
                         the cached list, Claude Code's own or the data file's"
                    ));
                }
            }
        }
    }
}

/// The list `load` reads from `file`, read again only when the file's mtime changed, and that
/// mtime in seconds. A missing file is no list.
fn by_mtime(
    file: &std::path::Path,
    held: &Held,
    load: impl FnOnce() -> Vec<LiveModel>,
) -> (Option<f64>, Vec<LiveModel>) {
    let modified = std::fs::metadata(file).and_then(|m| m.modified()).ok();
    let mut held = lock(held);
    let Some(modified) = modified else {
        *held = None;
        return (None, Vec::new());
    };
    if held.as_ref().is_none_or(|(at, _)| *at != modified) {
        *held = Some((modified, load()));
    }
    let secs = modified
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    (
        Some(secs),
        held.as_ref().map_or_else(Vec::new, |h| h.1.clone()),
    )
}

/// The network for a service that fetches nothing.
struct NoHttp;

impl HttpClient for NoHttp {
    fn send(&self, _: &HttpRequest) -> HttpResponse {
        HttpResponse {
            status: 0,
            body: Vec::new(),
        }
    }
}

/// A list read from a file, and the mtime of the file it came from.
type Held = Mutex<Option<(SystemTime, Vec<LiveModel>)>>;

pub struct CatalogService {
    inner: Arc<Inner>,
    codex_dir: PathBuf,
    /// Codex's list as read.
    codex: Held,
    /// Claude Code's main config file (`~/.claude.json`), whose `additionalModelOptionsCache` is
    /// Claude's list while Anthropic's has never been read. Read only.
    claude_code: Option<PathBuf>,
    /// Claude Code's list as read.
    claude_code_list: Held,
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
                refused: Mutex::new(None),
                log: Box::new(|line| eprintln!("{line}")),
                logged: AtomicBool::new(false),
            }),
            codex_dir,
            codex: Mutex::new(None),
            claude_code: None,
            claude_code_list: Mutex::new(None),
            spawner: thread_spawner(),
        }
    }

    /// Only the data file's models: no live list, no override, no accounts and no network. What
    /// the ops that need a catalog use until the daemon gives them its own.
    pub fn built_in() -> CatalogService {
        let nowhere = PathBuf::from("/nonexistent/notchling-catalog");
        CatalogService::new(
            Catalog::built_in(),
            LiveCache::new(nowhere.join("live-models.json")),
            nowhere.join("codex"),
            None,
            Arc::new(NoHttp),
        )
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
        let config = claude.0.env().config_file.clone();
        CatalogService::new(
            Catalog::load(&state.join("catalog.json")),
            LiveCache::new(state.join("live-models.json")),
            layout.codex_dir(),
            Some(claude),
            http,
        )
        .with_room(room)
        .with_claude_code_config(config)
    }

    /// Claude Code's main config file, whose own model list stands in for Anthropic's while that
    /// one has never been read (no token, or every fetch failed). Read only.
    pub fn with_claude_code_config(mut self, file: PathBuf) -> CatalogService {
        self.claude_code = Some(file);
        self
    }

    /// Where a failed fetch is logged (tests).
    pub fn with_log(mut self, log: Box<dyn Fn(&str) + Send + Sync>) -> CatalogService {
        self.inner_mut().log = log;
        self
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
        self.models_and_time(tool).0
    }

    /// Every model id a session of `tool` may start on.
    pub fn allowed(&self, tool: Tool) -> Vec<String> {
        self.inner.catalog.allowed(tool, &self.live(tool).1)
    }

    /// Whether a session of `tool` may start on `id`, as `Catalog::is_allowed` says it over the
    /// tool's live list.
    pub fn is_allowed(&self, tool: Tool, id: &str) -> bool {
        self.inner.catalog.is_allowed(tool, &self.live(tool).1, id)
    }

    /// A model's context window, as `catalog.get` reports it for that model: the catalog's (the
    /// data file, the override, or the family's it borrows) and, for a Claude model it has none
    /// for, the live list's; another tool's own list comes first. The phone's chat header shows
    /// this, so it shows what Chat's Context Meter does.
    pub fn context_window(&self, tool: Tool, model: &str) -> Option<u64> {
        let key = model_key(model);
        let live = || {
            self.live(tool)
                .1
                .into_iter()
                .find(|l| model_key(&l.id) == key)
                .and_then(|l| l.context_window)
        };
        let catalog = || self.inner.catalog.context_window(tool, model);
        match tool {
            Tool::Claude => catalog().or_else(live),
            _ => live().or_else(catalog),
        }
    }

    /// The models and when their live list was fetched or seen (`None` while only the data
    /// file's is shown), from one read of the list, so a fetch that lands meanwhile can't pair a
    /// new time with the old models.
    fn models_and_time(&self, tool: Tool) -> (Vec<ModelInfo>, Option<f64>) {
        let (at, live) = self.live(tool);
        (self.inner.catalog.models(tool, &live), at)
    }

    /// The tool's live list and when it was fetched, read once. Claude's is Anthropic's as last
    /// fetched, else Claude Code's own (`additionalModelOptionsCache`), else none.
    fn live(&self, tool: Tool) -> (Option<f64>, Vec<LiveModel>) {
        match tool {
            Tool::Codex => self.codex_list(),
            tool => match self.inner.cache.get(tool) {
                Some((at, models)) => (Some(at), models),
                None if tool == Tool::Claude => self.claude_code_models(),
                None => (None, Vec::new()),
            },
        }
    }

    /// Codex's list, read again only when its file's mtime changed, and that mtime in seconds.
    fn codex_list(&self) -> (Option<f64>, Vec<LiveModel>) {
        let file = self.codex_dir.join("models_cache.json");
        by_mtime(&file, &self.codex, || {
            codex_models::load(&self.codex_dir)
                .into_iter()
                .map(|m| LiveModel {
                    id: m.slug,
                    name: Some(m.name),
                    context_window: m.context_window,
                    efforts: m.efforts,
                    default_effort: m.default_effort,
                })
                .collect()
        })
    }

    /// Claude Code's own list, read again only when its config file's mtime changed (the file is
    /// large and changes often), and that mtime in seconds; none when it lists nothing.
    fn claude_code_models(&self) -> (Option<f64>, Vec<LiveModel>) {
        let Some(file) = &self.claude_code else {
            return (None, Vec::new());
        };
        let (at, models) = by_mtime(file, &self.claude_code_list, || claude_code::read(file));
        (at.filter(|_| !models.is_empty()), models)
    }

    /// Starts a fetch of Anthropic's list on a thread when it is due: none already running, the
    /// `accounts` switch on, the main login known, the cached list over a day old (or none), and
    /// the last try not within [`RETRY_SECS`]. Otherwise it does nothing. (A token refused within
    /// [`REFUSED_RETRY_SECS`] is checked on the thread, where the token is read.)
    pub fn refresh_claude(&self) {
        let inner = &self.inner;
        let Some((store, flag)) = &inner.claude else {
            return;
        };
        if !flag.is_on() {
            return;
        }
        // Nothing to do before the main login has been seen (the live token would be refused
        // anyway); this reads no Keychain.
        if store.snapshot().active.is_none() {
            return;
        }
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
            fetching.0.fetch_claude();
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
        let (models, fetched_at) = self.models_and_time(tool);
        Ok(json!({
            "tool": tool.name(),
            "models": models,
            "fetchedAt": fetched_at,
        }))
    }

    fn topics(&self) -> &[&'static str] {
        &[topics::TOOLS]
    }
}
