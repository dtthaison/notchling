use crate::layout::Layout;
use crate::search_redact;
use crate::search_scope::{Placing, Reach, AGENT_WINDOW_DAYS, POOL};
use crate::search_since;
use notchling_engine::control::session::home_of;
use notchling_engine::pages::{find_in, PageLibrary};
use notchling_engine::search::corpus_pages::page_key;
use notchling_engine::search::index::{
    AccountOf, ClaudeRoot, IndexRoots, SessionIndex, TitledSession,
};
use notchling_engine::search::query::category::{on, Category};
use notchling_engine::search::range::IndexRange;
use notchling_engine::search::{highlights, words, PageEntry, SessionHit, SessionMeta};
use notchling_engine::sessions::{config_dirs, Roots};
use notchling_local_api::{ops, ApiError, OpSpec};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub const PAGES_OPS: [&str; 4] = [
    ops::SEARCH_QUERY,
    ops::SEARCH_FACETS,
    ops::PAGES_LIST,
    ops::PAGES_FIND,
];
pub const ANSWERED: [Category; 2] = [Category::Sessions, Category::Pages];

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

#[derive(Clone, Debug, PartialEq)]
pub struct SearchSettings {
    pub range: IndexRange,
    pub on: Vec<Category>,
}

impl Default for SearchSettings {
    fn default() -> SearchSettings {
        SearchSettings {
            range: IndexRange::All,
            on: Category::ALL
                .into_iter()
                .filter(|c| c.default_on())
                .collect(),
        }
    }
}

impl SearchSettings {
    pub fn read(file: &std::path::Path) -> SearchSettings {
        let Ok(store) = notchling_engine::settings::Settings::open(file) else {
            return SearchSettings::default();
        };
        let list = |key: &str| -> Option<Vec<String>> {
            store.get(key).and_then(|v| {
                v.as_array().map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect()
                })
            })
        };
        let default = SearchSettings::default();
        SearchSettings {
            range: store
                .get("searchIndexRange")
                .and_then(|v| v.as_str().map(IndexRange::from_setting))
                .unwrap_or(default.range),
            on: list("searchCategories")
                .map(|ids| on(&ids))
                .unwrap_or(default.on),
        }
    }
}

pub type SearchRead = Arc<dyn Fn() -> SearchSettings + Send + Sync>;

pub type Titles = Arc<dyn Fn() -> Vec<TitledSession> + Send + Sync>;

pub struct PagesOps {
    library: PageLibrary,
    index: Arc<SessionIndex>,
    roots: Roots,
    codex_sessions: PathBuf,
    titles: Titles,
    accounts: Option<AccountOf>,
    settings: SearchRead,
    turn: Mutex<()>,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    embed: OnceLock<Arc<crate::embed_ops::EmbedOps>>,
}

pub(crate) fn system_now() -> f64 {
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
            index: Arc::new(SessionIndex::with_range(setup.cache, None)),
            roots: setup.roots,
            codex_sessions: setup.codex_sessions,
            titles: Arc::new(Vec::new),
            accounts: None,
            settings: Arc::new(SearchSettings::default),
            turn: Mutex::new(()),
            now: Arc::new(system_now),
            embed: OnceLock::new(),
        }
    }

    pub fn bind_embed(&self, embed: Arc<crate::embed_ops::EmbedOps>) {
        let _ = self.embed.set(embed);
    }

    pub fn embed_corpora(
        &self,
        categories: &[Category],
    ) -> Vec<Arc<dyn notchling_engine::search::corpus::Corpus>> {
        use notchling_engine::search::corpus::{PagesCorpus, SessionsCorpus};
        let mut out: Vec<Arc<dyn notchling_engine::search::corpus::Corpus>> = Vec::new();
        if categories.contains(&Category::Sessions) {
            let mut sessions = SessionsCorpus::new(self.index_roots());
            if let Some(accounts) = &self.accounts {
                sessions = sessions.with_accounts(accounts.clone());
            }
            out.push(Arc::new(sessions));
        }
        if categories.contains(&Category::Pages) {
            out.push(Arc::new(PagesCorpus::new(self.library.clone())));
        }
        out
    }

    pub fn with_titles(mut self, titles: Titles) -> PagesOps {
        self.titles = titles;
        self
    }

    pub fn with_accounts(mut self, accounts: AccountOf) -> PagesOps {
        self.accounts = Some(accounts.clone());
        if let Some(index) = Arc::into_inner(self.index) {
            self.index = Arc::new(index.with_accounts(accounts));
        } else {
            self.index = Arc::new(SessionIndex::with_range(None, None));
        }
        self
    }

    pub fn with_search_settings(mut self, read: SearchRead) -> PagesOps {
        self.settings = read;
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
        if op == ops::EMBED_REFRESH {
            return Some(OpSpec::APP);
        }
        PAGES_OPS.contains(&op).then_some(OpSpec::AGENT_READ)
    }

    pub fn poke(&self) {
        self.pass(None);
    }

    pub fn poke_and_wait(&self) {
        self.pass(Some(Duration::from_secs(20)));
    }

    fn pass(&self, wait: Option<Duration>) {
        let settings = self.settled();
        self.index.invalidate();
        if let Some(embed) = self.embed.get() {
            embed.refresh();
        }
        if settings.on.contains(&Category::Sessions) {
            self.refresh(wait);
        }
    }

    fn settled(&self) -> SearchSettings {
        let _turn = self.turn.lock().unwrap_or_else(|e| e.into_inner());
        let settings = (self.settings)();
        self.apply(&settings);
        settings
    }

    fn apply(&self, settings: &SearchSettings) {
        self.index.set_range(settings.range);
        let _ = self
            .index
            .set_sessions_on(settings.on.contains(&Category::Sessions));
        let _ = self
            .index
            .set_pages_on(settings.on.contains(&Category::Pages));
    }

    pub fn call(&self, op: &str, args: &Value) -> Result<Value, ApiError> {
        self.run(op, args, None)
    }

    pub fn call_scoped(
        &self,
        op: &str,
        args: &Value,
        org: &str,
        placing: &dyn Placing,
    ) -> Result<Value, ApiError> {
        let reach = Reach {
            org,
            placing,
            now: (self.now)(),
        };
        let mut reply = self.run(op, args, Some(&reach))?;
        reply["organization"] = json!(org);
        Ok(reply)
    }

    fn run(&self, op: &str, args: &Value, reach: Option<&Reach>) -> Result<Value, ApiError> {
        match op {
            ops::SEARCH_QUERY => self.search(args, reach),
            ops::SEARCH_FACETS => self.search_facets(args, reach),
            ops::EMBED_REFRESH => {
                self.poke();
                Ok(json!({}))
            }
            ops::PAGES_LIST => {
                let entries = self.reachable(self.library.entries(), reach);
                Ok(list_of(&self.library, &entries, reach.is_some()))
            }
            ops::PAGES_FIND => {
                let query = args
                    .get("query")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ApiError::bad_request("pages.find takes {\"query\": string}"))?;
                let entries = self.reachable(self.library.entries(), reach);
                let found = facets::found_in(query, &entries, reach.is_some());
                Ok(find_of(&self.library, &found, reach.is_some()))
            }
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }

    fn reachable(&self, entries: Vec<PageEntry>, reach: Option<&Reach>) -> Vec<PageEntry> {
        match reach {
            Some(reach) => entries.into_iter().filter(|e| reach.page(e)).collect(),
            None => entries,
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

    fn content_hits(
        &self,
        entries: &[PageEntry],
        query_words: &[String],
        kind: Option<&str>,
        now: f64,
    ) -> Vec<PageEntry> {
        if query_words.is_empty() || kind == Some("sessions") {
            return Vec::new();
        }
        self.index.refresh_pages(&self.library);
        let by_key: HashMap<String, &PageEntry> =
            entries.iter().map(|e| (page_key(e), e)).collect();
        self.index
            .search_pages(query_words, now, POOL)
            .into_iter()
            .filter_map(|h| by_key.get(&h.key).map(|e| (*e).clone()))
            .collect()
    }
}

pub fn page_json(lib: &PageLibrary, e: &PageEntry) -> Value {
    page_json_for(lib, e, false)
}

fn page_json_for(lib: &PageLibrary, e: &PageEntry, masked: bool) -> Value {
    let (title, account) = if masked {
        (
            search_redact::mask(&e.title),
            search_redact::mask(&e.account),
        )
    } else {
        (e.title.clone(), e.account.clone())
    };
    json!({
        "id": e.artifact_id(),
        "title": title,
        "url": e.url,
        "project": e.project,
        "account": account,
        "updated": e.updated,
        "version": e.version,
        "folder": e.folder,
        "file": e.file,
        "source": e.source,
        "path": lib.local_path(e).map(|p| p.to_string_lossy().into_owned()),
        "facets": facets::json_of(&facets::page_facets(e, masked)),
    })
}

pub fn session_json(hit: &SessionHit, query_words: &[String]) -> Value {
    session_json_for(hit, query_words, false)
}

fn marked(
    snippet: Option<String>,
    query_words: &[String],
    masked: bool,
) -> (Option<String>, Vec<Value>) {
    let Some(text) = snippet else {
        return (None, Vec::new());
    };
    let mut marks: Vec<Range<usize>> = highlights(&text, query_words);
    let mut text = text;
    if masked {
        let (hidden, moved) = search_redact::mask_marked(&text, &marks);
        text = hidden;
        marks = moved;
    }
    let marks = marks.into_iter().map(|r| json!([r.start, r.end])).collect();
    (Some(text), marks)
}

fn session_json_for(hit: &SessionHit, query_words: &[String], masked: bool) -> Value {
    let (snippet, marks) = marked(hit.snippet.clone(), query_words, masked);
    let title = if masked {
        search_redact::mask(&hit.meta.title)
    } else {
        hit.meta.title.clone()
    };
    json!({
        "key": hit.meta.key,
        "tool": hit.meta.tool_key,
        "session_id": hit.meta.session_id,
        "title": title,
        "folder": hit.meta.folder,
        "path": hit.meta.path,
        "config_dir": hit.meta.config_dir,
        "facets": facets::json_of(&facets::session_facets(&hit.meta, masked)),
        "parent": hit.meta.parent,
        "modified": hit.meta.modified,
        "snippet": snippet,
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
    list_of(lib, &lib.entries(), false)
}

pub fn find_value(lib: &PageLibrary, query: &str) -> Value {
    find_of(lib, &lib.find(query), false)
}

fn list_of(lib: &PageLibrary, entries: &[PageEntry], masked: bool) -> Value {
    let (pages, truncated) = capped(entries, |e| page_json_for(lib, e, masked));
    json!({"pages": pages, "truncated": truncated})
}

fn find_of(lib: &PageLibrary, entries: &[PageEntry], masked: bool) -> Value {
    let (pages, truncated) = capped(entries, |e| {
        let mut value = page_json_for(lib, e, masked);
        if let Some(map) = value.as_object_mut() {
            let mut describe = lib.describe(e);
            if masked {
                describe = describe.replace(&e.title, &search_redact::mask(&e.title));
                if !e.account.is_empty() {
                    describe = describe.replace(&e.account, &search_redact::mask(&e.account));
                }
            }
            map.insert("describe".into(), Value::String(describe));
        }
        value
    });
    json!({"pages": pages, "truncated": truncated})
}

#[path = "pages_ops_facets.rs"]
mod facets;
#[path = "search_query.rs"]
mod search_query;
