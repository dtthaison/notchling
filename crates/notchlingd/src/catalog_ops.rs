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

pub const RETRY_SECS: f64 = 900.0;

pub const REFUSED_RETRY_SECS: f64 = 86_400.0;

pub type Spawner = Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_secs() -> f64 {
    now_ms() as f64 / 1000.0
}

fn thread_spawner() -> Spawner {
    Arc::new(|job| {
        let _ = std::thread::Builder::new()
            .name("notchling-catalog".into())
            .spawn(job);
    })
}

struct Fetching(Arc<Inner>);

impl Drop for Fetching {
    fn drop(&mut self) {
        self.0.fetching.store(false, Ordering::SeqCst);
    }
}

struct Inner {
    catalog: Arc<Catalog>,
    cache: LiveCache,
    claude: Option<(Arc<ClaudeAccounts>, Arc<SwitchFlag>)>,
    http: Arc<dyn HttpClient>,
    room: Option<Arc<dyn Room>>,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    fetching: AtomicBool,
    last_try: Mutex<Option<f64>>,
    refused: Mutex<Option<(f64, [u8; 32])>>,
    log: Box<dyn Fn(&str) + Send + Sync>,
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

struct NoHttp;

impl HttpClient for NoHttp {
    fn send(&self, _: &HttpRequest) -> HttpResponse {
        HttpResponse {
            status: 0,
            body: Vec::new(),
        }
    }
}

type Held = Mutex<Option<(SystemTime, Vec<LiveModel>)>>;

pub struct CatalogService {
    inner: Arc<Inner>,
    codex_dir: PathBuf,
    codex: Held,
    claude_code: Option<PathBuf>,
    claude_code_list: Held,
    spawner: Spawner,
}

impl CatalogService {
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

    pub fn with_claude_code_config(mut self, file: PathBuf) -> CatalogService {
        self.claude_code = Some(file);
        self
    }

    pub fn with_log(mut self, log: Box<dyn Fn(&str) + Send + Sync>) -> CatalogService {
        self.inner_mut().log = log;
        self
    }

    pub fn with_room(mut self, room: Arc<dyn Room>) -> CatalogService {
        self.inner_mut().room = Some(room);
        self
    }

    pub fn with_clock(mut self, now: Arc<dyn Fn() -> f64 + Send + Sync>) -> CatalogService {
        self.inner_mut().now = now;
        self
    }

    pub fn with_spawner(mut self, spawner: Spawner) -> CatalogService {
        self.spawner = spawner;
        self
    }

    fn inner_mut(&mut self) -> &mut Inner {
        Arc::get_mut(&mut self.inner).expect("the service is configured before it is shared")
    }

    pub fn catalog(&self) -> &Catalog {
        &self.inner.catalog
    }

    pub fn shared_catalog(&self) -> Arc<Catalog> {
        self.inner.catalog.clone()
    }

    pub fn models(&self, tool: Tool) -> Vec<ModelInfo> {
        self.models_and_time(tool).0
    }

    pub fn allowed(&self, tool: Tool) -> Vec<String> {
        self.inner.catalog.allowed(tool, &self.live(tool).1)
    }

    pub fn is_allowed(&self, tool: Tool, id: &str) -> bool {
        self.inner.catalog.is_allowed(tool, &self.live(tool).1, id)
    }

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

    fn models_and_time(&self, tool: Tool) -> (Vec<ModelInfo>, Option<f64>) {
        let (at, live) = self.live(tool);
        (self.inner.catalog.models(tool, &live), at)
    }

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

    fn claude_code_models(&self) -> (Option<f64>, Vec<LiveModel>) {
        let Some(file) = &self.claude_code else {
            return (None, Vec::new());
        };
        let (at, models) = by_mtime(file, &self.claude_code_list, || claude_code::read(file));
        (at.filter(|_| !models.is_empty()), models)
    }

    pub fn refresh_claude(&self) {
        let inner = &self.inner;
        let Some((store, flag)) = &inner.claude else {
            return;
        };
        if !flag.is_on() {
            return;
        }
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
