use crate::acting_ops::{now_ms, Room};
use crate::layout::Layout;
use crate::op_group::OpGroup;
use crate::switches::SwitchFlag;
use notchling_engine::accounts::claude::ClaudeAccounts;
use notchling_engine::accounts::http::{HttpClient, HttpRequest, HttpResponse};
use notchling_engine::catalog::live_cache::{refresh_due, LiveCache};
use notchling_engine::catalog::roles::Roles;
use notchling_engine::catalog::{claude_code, claude_live};
use notchling_engine::catalog::{model_key, override_file, Catalog, LiveModel, ModelInfo, Tool};
use notchling_engine::delegate::role_models::{codex_hints_with, Live};
use notchling_engine::delegate::role_view;
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

pub const MANUAL_MIN_SECS: f64 = 60.0;

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
    last_manual: Mutex<Option<f64>>,
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
        if self.refused_now(&token) {
            return;
        }
        match self.ask_claude(&token) {
            Ok(models) => {
                self.cache.put(Tool::Claude, (self.now)(), &models);
                self.announce(Tool::Claude);
            }
            Err(why) => {
                if !self.logged.swap(true, Ordering::SeqCst) {
                    (self.log)(&format!(
                        "notchling: Claude's model list wasn't read ({why}); the pickers keep \
                         the list they have: the last one read, else Claude Code's own, else \
                         the data file's"
                    ));
                }
            }
        }
    }

    fn refused_now(&self, token: &notchling_engine::accounts::Secret) -> bool {
        let fingerprint = claude_live::token_fingerprint(token);
        let now = (self.now)();
        lock(&self.refused).is_some_and(|(at, seen)| {
            seen == fingerprint && now >= at && now - at < REFUSED_RETRY_SECS
        })
    }

    fn ask_claude(
        &self,
        token: &notchling_engine::accounts::Secret,
    ) -> Result<Vec<LiveModel>, String> {
        let fingerprint = claude_live::token_fingerprint(token);
        let now = (self.now)();
        let answer = claude_live::fetch(&*self.http, token);
        *lock(&self.refused) = match &answer {
            Err(why) if why.refused() => Some((now, fingerprint)),
            _ => None,
        };
        match answer {
            Ok(models) if models.is_empty() => Err("Anthropic listed no models".into()),
            Ok(models) => Ok(models),
            Err(why) => Err(why.to_string()),
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
    codex_snapshot: Option<PathBuf>,
    codex: Held,
    claude_code: Option<PathBuf>,
    claude_code_list: Held,
    override_file: Option<PathBuf>,
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
                last_manual: Mutex::new(None),
                log: Box::new(|line| eprintln!("{line}")),
                logged: AtomicBool::new(false),
            }),
            codex_dir,
            codex_snapshot: None,
            codex: Mutex::new(None),
            claude_code: None,
            claude_code_list: Mutex::new(None),
            override_file: None,
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
        .with_override_file(state.join("catalog.json"))
        .with_codex_snapshot(state.join(codex_models::SNAPSHOT))
    }

    pub fn with_codex_snapshot(mut self, file: PathBuf) -> CatalogService {
        self.codex_snapshot = Some(file);
        self
    }

    pub fn override_path(&self) -> Option<&std::path::Path> {
        self.override_file.as_deref()
    }

    pub fn codex_dir(&self) -> &std::path::Path {
        &self.codex_dir
    }

    pub fn codex_snapshot_path(&self) -> Option<&std::path::Path> {
        self.codex_snapshot.as_deref()
    }

    pub fn live_models(&self, tool: Tool) -> Vec<LiveModel> {
        self.live(tool).1
    }

    pub fn store_live(&self, tool: Tool, models: &[LiveModel]) {
        let inner = &self.inner;
        let changed = inner.cache.get(tool).is_none_or(|(_, old)| old != models);
        inner.cache.put(tool, (inner.now)(), models);
        if changed {
            inner.announce(tool);
        }
    }

    pub fn announce_changed(&self, tool: Tool) {
        self.inner.announce(tool);
    }

    pub fn read_claude_now(&self) -> Result<Vec<LiveModel>, String> {
        let Some((store, flag)) = &self.inner.claude else {
            return Err("Claude accounts aren't available".into());
        };
        if !flag.is_on() {
            return Err("the accounts switch is off".into());
        }
        let token = store
            .live_token()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no Claude Code login is live".to_string())?;
        let inner = &self.inner;
        if inner.refused_now(&token) {
            return Err("Anthropic refused this login a short while ago; it isn't asked again until tomorrow".into());
        }
        let now = (inner.now)();
        {
            let mut last = lock(&inner.last_manual);
            if last.is_some_and(|at| now >= at && now - at < MANUAL_MIN_SECS) {
                return Err("Anthropic was asked less than a minute ago".into());
            }
            *last = Some(now);
        }
        inner.ask_claude(&token)
    }

    pub fn with_override_file(mut self, file: PathBuf) -> CatalogService {
        self.override_file = Some(file);
        self
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
        let snapshot = self.codex_snapshot.as_deref();
        let file = codex_models::source_file(&self.codex_dir, snapshot);
        by_mtime(&file, &self.codex, || {
            codex_models::load_with(&self.codex_dir, snapshot)
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
        let inner = &self.inner;
        let before = inner
            .cache
            .get(Tool::Antigravity)
            .map(|(_, old)| old)
            .unwrap_or_default();
        let models: Vec<LiveModel> = model
            .values
            .iter()
            .map(|v| {
                let known = before.iter().find(|b| b.id == v.value).filter(|b| {
                    !b.efforts.is_empty() && efforts.iter().all(|e| b.efforts.contains(e))
                });
                LiveModel {
                    id: v.value.clone(),
                    name: Some(v.name.clone()),
                    efforts: known.map_or_else(|| efforts.clone(), |b| b.efforts.clone()),
                    default_effort: default_effort.clone(),
                    ..LiveModel::default()
                }
            })
            .collect();
        let changed = before != models;
        inner.cache.put(Tool::Antigravity, (inner.now)(), &models);
        if changed {
            inner.announce(Tool::Antigravity);
        }
    }
}

impl CatalogService {
    fn roles(&self) -> Value {
        let file = self.override_file.as_deref();
        let roles = Roles::load(file);
        let live = |tool: Tool, hints: Vec<(String, String)>| {
            let models = self.live(tool).1;
            Live {
                models: models.iter().map(|m| m.id.clone()).collect(),
                hints,
                efforts: models.into_iter().map(|m| (m.id, m.efforts)).collect(),
            }
        };
        let codex = live(
            Tool::Codex,
            codex_hints_with(&self.codex_dir, self.codex_snapshot.as_deref()),
        );
        let agy = live(Tool::Antigravity, Vec::new());
        let views = role_view::views(&roles, &Roles::built_in(), &codex, &agy);
        json!({
            "roles": role_view::to_json(&views),
            "override": {
                "path": file.map(|f| f.to_string_lossy().into_owned()),
                "state": file.map_or("missing", |f| override_file::state(f).as_str()),
            },
        })
    }
}

impl OpGroup for CatalogService {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::CATALOG_GET => Some(OpSpec::AGENT),
            ops::CATALOG_ROLES => Some(OpSpec::APP_QUIET),
            _ => None,
        }
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if op == ops::CATALOG_ROLES {
            return Ok(self.roles());
        }
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
