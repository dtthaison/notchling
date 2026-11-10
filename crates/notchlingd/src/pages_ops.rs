//! E7 in the daemon: the pages library and the session index as agent-tier read ops (Milestone 5).
//! Reads always work, whatever the `acting` switch says; nothing here acts or writes the Swift
//! app's folders.
//!
//! | Op | Args | Reply |
//! | --- | --- | --- |
//! | `search.query` | `{text?: string, limit?: int}` | `{"pages": [Page], "sessions": [Session], "indexing": bool}` |
//! | `pages.list` | `{}` | `{"pages": [Page], "truncated": bool}` |
//! | `pages.find` | `{query: string}` | `{"pages": [Page + {"describe": string}], "truncated": bool}` |
//!
//! `Page` (the fields of `pages.json`, plus two):
//!
//! ```json
//! {"id": "AbCdEfGhIjKlMnOpQrStUv", "title": "Cost Report", "url": "https://claude.ai/artifact/AbCd...",
//!  "project": "notchling", "account": "me@example.com", "updated": 1794150245.0,
//!  "version": "1793506664-65e3", "folder": "cost-report-1a2b3c4d", "file": "index.html",
//!  "source": "/w/report.html", "path": "/.../pages/cost-report-1a2b3c4d/index.html"}
//! ```
//!
//! - `id` is the last part of the claude.ai link, `null` for a page that never got a link.
//! - `url`, `source`, `project`, `account` and `version` are `""` when unknown.
//! - `updated` is seconds since 1970. Pages come newest first.
//! - `path` is the page's local copy, absolute; `null` when the stored folder or file name is not
//!   a plain name. It is what the pages window loads. `index.html` beside the folders is the
//!   library's own list page.
//!
//! `Session`:
//!
//! ```json
//! {"key": "claude:ab12", "tool": "claude" | "codex" | "agy", "session_id": "ab12",
//!  "title": "Token cache work", "folder": "/w/app", "path": "/.../ab12.jsonl",
//!  "config_dir": "/Users/me/.claude-lanes/work" | null, "modified": 1794150245.0,
//!  "snippet": "...the stale token cache..." | null, "highlights": [[4, 9]], "score": 61.5}
//! ```
//!
//! - `key` is Chat's row id (`claude:<id>`, `codex:<id>`, `agy:<id>`); Return on a session opens it
//!   with that.
//! - `path` is `""` for Antigravity (found by title). `config_dir` is the lane's folder, `null` for
//!   `~/.claude`.
//! - `snippet` is a cut of the text around the match, `null` when only the title or folder
//!   matched or there was no query; `highlights` are byte ranges `[start, end)` into the snippet.
//!
//! `search.query`:
//! - `text` is split on whitespace; a page needs every word in its title, project, account or
//!   link (a pasted link or id finds its page); a session needs every word in its title, folder
//!   or text. `text` empty or missing lists the newest of each.
//! - Pages: words in the title first, newest first within each; at most 20 (6 for an empty
//!   `text`), and no more than `limit`. Sessions: at most `limit` (default 30, 6 for an empty
//!   `text`; capped at 200).
//! - The reply is the index as it is: a refresh runs in the background when the last one is 8 s
//!   old or more, and `indexing` says one is running, so a client asks again a moment later. The
//!   very first query waits up to 2 s for the first refresh.
//!
//! `pages.find`: the page with this link or id, else the pages holding every word (an empty query
//! is every page). `describe` is the text `notchling page` prints.
//!
//! A reply is one line and a line is at most 1 MiB, so `pages.list` and `pages.find` stop at 1000
//! pages or 700 KB and say `"truncated": true`.

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

/// Every op here; all agent tier.
pub const PAGES_OPS: [&str; 3] = [ops::SEARCH_QUERY, ops::PAGES_LIST, ops::PAGES_FIND];

/// A refresh runs for a query when the last one ended this long ago.
pub const REFRESH_EVERY_SECS: f64 = 8.0;
/// How long the very first query waits for the first refresh.
pub const FIRST_REFRESH_WAIT: Duration = Duration::from_secs(2);
/// Most sessions one query returns.
pub const MAX_SESSIONS: usize = 200;
/// Most pages `pages.list` and `pages.find` return, and the size they stop at.
pub const MAX_PAGES: usize = 1000;
pub const MAX_PAGES_BYTES: usize = 700_000;

/// Where everything is read from.
#[derive(Clone, Debug)]
pub struct PagesSetup {
    /// The pages library (`<data dir>/pages`).
    pub pages_dir: PathBuf,
    /// True only for a test folder; the daemon's ops never write either way.
    pub writable: bool,
    /// `~/.claude` and the lanes.
    pub roots: Roots,
    /// Codex's `sessions` folder.
    pub codex_sessions: PathBuf,
    /// The daemon's own index cache; `None` keeps it in memory.
    pub cache: Option<PathBuf>,
}

impl PagesSetup {
    /// From the daemon's layout. The library is the data folder's `pages`, which on a real run is
    /// the Swift app's folder, so it is read-only. The cache is the daemon's own file
    /// (`<data dir>/daemon/search.db`), never the Swift app's `search-index.json`; a
    /// daemon given `--socket` (a test) keeps none.
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

/// Where Antigravity's session titles come from (E6b's list); none until then.
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

    /// Antigravity's titles for the index (E6b supplies them).
    pub fn with_titles(mut self, titles: Titles) -> PagesOps {
        self.titles = titles;
        self
    }

    /// A fixed clock (tests).
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

    /// The roots the index reads now: every config folder's `projects`, Codex's rollouts and the
    /// Antigravity titles.
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

    /// Starts a refresh in the background if one is due. With `wait`, returns after it ends or
    /// after that long. A refresh already running is left to finish.
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

        // Answer from the cache first; the very first query waits a moment for a real read.
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

/// A page as the ops return it.
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

/// A session hit as the ops return it.
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

/// The pages as JSON objects, stopping at the line budget.
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

/// `pages.list`'s reply for a library.
pub fn list_value(lib: &PageLibrary) -> Value {
    let (pages, truncated) = capped(&lib.entries(), |e| page_json(lib, e));
    json!({"pages": pages, "truncated": truncated})
}

/// `pages.find`'s reply for a library. The client without a daemon builds the same reply.
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
