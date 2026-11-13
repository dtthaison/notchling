use crate::layout::Layout;
use notchling_engine::control::session::home_of;
use notchling_engine::pages::{find_in, PageLibrary};
use notchling_engine::search::index::{ClaudeRoot, IndexRoots, SessionIndex, TitledSession};
use notchling_engine::search::{highlights, words, PageEntry, SessionHit};
use notchling_engine::sessions::{config_dirs, Roots};
use notchling_local_api::{ops, ApiError, OpSpec};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

pub const PAGES_OPS: [&str; 3] = [ops::SEARCH_QUERY, ops::PAGES_LIST, ops::PAGES_FIND];

pub const REFRESH_EVERY_SECS: f64 = 8.0;
pub const FIRST_REFRESH_WAIT: Duration = Duration::from_secs(2);
pub const MAX_SESSIONS: usize = 200;
pub const MAX_PAGES: usize = 1000;
pub const MAX_PAGES_BYTES: usize = 700_000;

#[derive(Clone, Debug)]
pub struct PagesSetup {
    pub pages_dir: PathBuf,
    pub writable: bool,
    pub roots: Roots,
    pub codex_sessions: PathBuf,
    pub cache: Option<PathBuf>,
}

impl PagesSetup {
    pub fn from_layout(layout: &Layout) -> PagesSetup {
        let roots = layout.roots();
        let codex_home = if layout.fixture {
            layout
                .claude_dir
                .parent()
                .map(|p| p.join("codex"))
                .unwrap_or_else(|| layout.claude_dir.join("codex"))
        } else {
            std::env::var_os("CODEX_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(home_of(&roots)).join(".codex"))
        };
        PagesSetup {
            pages_dir: layout.data_dir.join("pages"),
            writable: layout.fixture,
            roots,
            codex_sessions: codex_home.join("sessions"),
            cache: (!layout.socket_given).then(|| layout.data_dir.join("daemon").join("search.db")),
        }
    }
}

pub type Titles = Arc<dyn Fn() -> Vec<TitledSession> + Send + Sync>;

pub struct PagesOps {
    library: PageLibrary,
    index: Arc<SessionIndex>,
    roots: Roots,
    codex_sessions: PathBuf,
    titles: Titles,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
}

fn system_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl PagesOps {
    pub fn new(setup: PagesSetup) -> PagesOps {
        let library = if setup.writable {
            PageLibrary::writable(&setup.pages_dir)
        } else {
            PageLibrary::read_only(&setup.pages_dir)
        };
        PagesOps {
            library,
            index: Arc::new(SessionIndex::new(setup.cache)),
            roots: setup.roots,
            codex_sessions: setup.codex_sessions,
            titles: Arc::new(Vec::new),
            now: Arc::new(system_now),
        }
    }

    pub fn with_titles(mut self, titles: Titles) -> PagesOps {
        self.titles = titles;
        self
    }

    pub fn with_clock(mut self, now: Arc<dyn Fn() -> f64 + Send + Sync>) -> PagesOps {
        self.now = now;
        self
    }

    pub fn library(&self) -> &PageLibrary {
        &self.library
    }

    pub fn index(&self) -> &SessionIndex {
        &self.index
    }

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        PAGES_OPS.contains(&op).then_some(OpSpec::AGENT)
    }

    pub fn call(&self, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::SEARCH_QUERY => self.search(args),
            ops::PAGES_LIST => Ok(list_value(&self.library)),
            ops::PAGES_FIND => {
                let query = args
                    .get("query")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ApiError::bad_request("pages.find takes {\"query\": string}"))?;
                Ok(find_value(&self.library, query))
            }
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }

    fn index_roots(&self) -> IndexRoots {
        IndexRoots {
            claude: config_dirs(&self.roots)
                .into_iter()
                .map(|c| ClaudeRoot {
                    projects: c.dir.join("projects"),
                    config_dir: c.lane.map(|l| l.path),
                })
                .collect(),
            codex_sessions: Some(self.codex_sessions.clone()),
            titled: (self.titles)(),
        }
    }

    fn refresh(&self, wait: Option<Duration>) {
        let now = (self.now)();
        if !self.index.due(now, REFRESH_EVERY_SECS) {
            return;
        }
        let roots = self.index_roots();
        let index = self.index.clone();
        let (done, finished) = channel();
        std::thread::spawn(move || {
            index.refresh(&roots, now);
            let _ = done.send(());
        });
        if let Some(wait) = wait {
            match finished.recv_timeout(wait) {
                Ok(()) | Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
            }
        }
    }

    fn search(&self, args: &Value) -> Result<Value, ApiError> {
        let text = match args.get("text") {
            None | Some(Value::Null) => "",
            Some(Value::String(s)) => s.as_str(),
            Some(_) => {
                return Err(ApiError::bad_request(
                    "search.query takes {\"text\": string, \"limit\": int}",
                ))
            }
        };
        let limit = match args.get("limit") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| {
                ApiError::bad_request("search.query takes {\"text\": string, \"limit\": int}")
            })? as usize),
        };
        let query_words = words(text);
        let empty = query_words.is_empty();
        let session_limit = limit
            .unwrap_or(if empty { 6 } else { 30 })
            .min(MAX_SESSIONS);
        let page_limit = (if empty { 6 } else { 20 }).min(limit.unwrap_or(usize::MAX));

        let first = self.index.is_empty() && !self.index.is_refreshing();
        self.index.load_cache();
        self.refresh(first.then_some(FIRST_REFRESH_WAIT));

        let entries = self.library.entries();
        let found = find_in(text, &entries);
        let lowered: Vec<String> = words(text);
        let (in_title, rest): (Vec<PageEntry>, Vec<PageEntry>) = found.into_iter().partition(|p| {
            let title = notchling_engine::search::lowercase(&p.title);
            lowered.iter().all(|w| title.contains(w.as_str()))
        });
        let pages: Vec<Value> = in_title
            .iter()
            .chain(rest.iter())
            .take(page_limit)
            .map(|p| page_json(&self.library, p))
            .collect();
        let sessions: Vec<Value> = self
            .index
            .search(&query_words, (self.now)(), session_limit)
            .iter()
            .map(|hit| session_json(hit, &query_words))
            .collect();
        Ok(json!({
            "pages": pages,
            "sessions": sessions,
            "indexing": self.index.is_refreshing(),
        }))
    }
}

pub fn page_json(lib: &PageLibrary, e: &PageEntry) -> Value {
    json!({
        "id": e.artifact_id(),
        "title": e.title,
        "url": e.url,
        "project": e.project,
        "account": e.account,
        "updated": e.updated,
        "version": e.version,
        "folder": e.folder,
        "file": e.file,
        "source": e.source,
        "path": lib.local_path(e).map(|p| p.to_string_lossy().into_owned()),
    })
}

pub fn session_json(hit: &SessionHit, query_words: &[String]) -> Value {
    let marks: Vec<Value> = hit
        .snippet
        .as_deref()
        .map(|s| {
            highlights(s, query_words)
                .into_iter()
                .map(|r| json!([r.start, r.end]))
                .collect()
        })
        .unwrap_or_default();
    json!({
        "key": hit.meta.key,
        "tool": hit.meta.tool_key,
        "session_id": hit.meta.session_id,
        "title": hit.meta.title,
        "folder": hit.meta.folder,
        "path": hit.meta.path,
        "config_dir": hit.meta.config_dir,
        "modified": hit.meta.modified,
        "snippet": hit.snippet,
        "highlights": marks,
        "score": hit.score,
    })
}

fn capped(entries: &[PageEntry], page: impl Fn(&PageEntry) -> Value) -> (Vec<Value>, bool) {
    let mut out = Vec::new();
    let mut bytes = 0;
    for e in entries {
        let value = page(e);
        bytes += value.to_string().len() + 1;
        if out.len() >= MAX_PAGES || bytes > MAX_PAGES_BYTES {
            return (out, true);
        }
        out.push(value);
    }
    (out, false)
}

pub fn list_value(lib: &PageLibrary) -> Value {
    let (pages, truncated) = capped(&lib.entries(), |e| page_json(lib, e));
    json!({"pages": pages, "truncated": truncated})
}

pub fn find_value(lib: &PageLibrary, query: &str) -> Value {
    let (pages, truncated) = capped(&lib.find(query), |e| {
        let mut value = page_json(lib, e);
        if let Some(map) = value.as_object_mut() {
            map.insert("describe".into(), Value::String(lib.describe(e)));
        }
        value
    });
    json!({"pages": pages, "truncated": truncated})
}
