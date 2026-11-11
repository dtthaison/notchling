//! E8c in the daemon: `cost.summary`, the token cost over the transcripts. Agent tier, read only,
//! not switched: it reads transcripts and nothing else (and the account log, for the CLI's
//! account), and writes only the daemon's own cache.
//!
//! | Op | Tier | Args | Reply |
//! | --- | --- | --- | --- |
//! | `cost.summary` | agent | `{range?: "7" or "30" or "all"}` (30 by default; the numbers 7 and 30 too) | see below |
//!
//! ```json
//! {"range": "30", "scanned": 1794150245.0, "scanning": false,
//!  "summary": {"days": [{"day": "2026-11-08", "cost": 1.5}], "today": 1.5, "week": 9.0, "month": 31.0,
//!             "models": [{"name": "Opus 5.5", "cost": 30.0}],
//!             "model_detail": [{"name": "Opus 5.5", "cost": 30.0, "percent": 96.8, "input": 1.0,
//!                               "output": 2.0, "cache_read": 3.0, "cache_write": 4.0}],
//!             "projects": [{"name": "notchling", "cost": 20.0}],
//!             "accounts": [{"id": "uuid:uuid", "cost": 25.0}],
//!             "channels": [{"name": "Claude Code", "cost": 30.0}],
//!             "channel_tokens": {"Claude Code": 123456.0},
//!             "unpriced": ["claude-new-9"], "borrowed": ["claude-fable-5-3"]},
//!  "by_account": {"uuid:uuid": {...same as summary...}}}
//! ```
//!
//! - Costs are dollars at API list prices. `week` and `today` are the same in every range;
//!   `month` is the range's total.
//! - The reply is what the last scan published. A scan starts on a thread when a request finds the
//!   last one 20 s old or none; `scanned` is when the last finished one began (`null` before the
//!   first) and `scanning` says one is running, so a client asks again a moment later.
//! - Prices come from the daemon's one catalog ([`crate::catalog_ops::CatalogService`]: the
//!   built-in data, with the override file `catalog.json` beside the socket laid over it).
//!   `unpriced` lists the models in the range that have no price: their tokens are counted and
//!   their dollars are 0, and the daemon logs each such id once per run ([`UnpricedLog`]).
//!   `borrowed` lists those newer than any the catalog knows, priced as the newest of their
//!   family. Both are sorted ids, once each.
//! - The lists are cut to 50 rows, and `by_account` is left out if the reply would pass 700 KB
//!   (a line is at most 1 MiB).

use crate::layout::Layout;
use notchling_engine::catalog::Catalog;
use notchling_engine::cost::summary::Summary;
use notchling_engine::cost::tracker::{Root, ANTIGRAVITY, CLAUDE_CODE, CODEX, DESKTOP};
use notchling_engine::cost::{CostService, Snapshot, Tracker, Window};
use notchling_local_api::ApiError;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Rows kept per list in a reply.
pub const MAX_ROWS: usize = 50;
/// The reply's size, under a line's 1 MiB.
pub const MAX_REPLY_BYTES: usize = 700_000;

/// Where the transcripts are read from and the cache kept.
#[derive(Clone, Debug)]
pub struct CostSetup {
    pub roots: Vec<Root>,
    /// The data folder whose `notchling.db` holds the daemon's own cache (the `daemon/cost-cache`
    /// document); `None` keeps it in memory.
    pub cache_data: Option<PathBuf>,
    /// The account store's `account-log.json`, read only.
    pub account_log: PathBuf,
    /// The catalog prices come from: the daemon's one, shared with `catalog.get`, not a copy
    /// loaded here.
    pub catalog: Arc<Catalog>,
}

impl CostSetup {
    /// Claude Code's `projects`, the Claude desktop app's two session folders, Codex's `sessions`
    /// and Antigravity's `brain`, all read only. The cache is the daemon's own, never the Swift
    /// app's `cost-cache.json`; a daemon given `--socket` (a test) keeps none.
    pub fn from_layout(layout: &Layout, catalog: Arc<Catalog>) -> CostSetup {
        let desktop = layout.home().join("Library/Application Support/Claude");
        let root = |dir: PathBuf, channel| Root { dir, channel };
        CostSetup {
            roots: vec![
                root(layout.claude_dir.join("projects"), CLAUDE_CODE),
                root(desktop.join("local-agent-mode-sessions"), DESKTOP),
                root(desktop.join("claude-code-sessions"), DESKTOP),
                root(layout.codex_dir().join("sessions"), CODEX),
                root(layout.antigravity_dir().join("brain"), ANTIGRAVITY),
            ],
            cache_data: (!layout.socket_given).then(|| layout.data_dir.clone()),
            account_log: layout.data_dir.join("account-log.json"),
            catalog,
        }
    }

    pub fn service(&self) -> Arc<CostService> {
        CostService::new(Tracker::new(
            self.roots.clone(),
            self.cache_data.clone(),
            Some(self.account_log.clone()),
            self.catalog.clone(),
        ))
    }
}

fn range_of(args: &Value) -> Result<Window, ApiError> {
    let bad = || ApiError::bad_request("range is \"7\", \"30\" or \"all\"");
    match args.get("range") {
        None | Some(Value::Null) => Ok(Window::Days(30)),
        Some(Value::String(s)) => Window::parse(s).ok_or_else(bad),
        Some(Value::Number(n)) => Window::parse(&n.to_string()).ok_or_else(bad),
        Some(_) => Err(bad()),
    }
}

fn cut(summary: &Summary) -> Value {
    let mut s = summary.clone();
    s.models.truncate(MAX_ROWS);
    s.model_detail.truncate(MAX_ROWS);
    s.projects.truncate(MAX_ROWS);
    s.accounts.truncate(MAX_ROWS);
    s.channels.truncate(MAX_ROWS);
    serde_json::to_value(s).unwrap_or(Value::Null)
}

/// The reply for one request, from what the service has published.
pub fn reply(snapshot: &Snapshot, window: Window) -> Value {
    let summary = snapshot.ranged.overall.get(&window);
    let mut out = Map::new();
    out.insert("range".into(), json!(window.name()));
    out.insert("scanned".into(), json!(snapshot.scanned));
    out.insert("scanning".into(), json!(snapshot.scanning));
    out.insert(
        "summary".into(),
        summary.map_or_else(|| cut(&Summary::default()), cut),
    );
    let by_account: Map<String, Value> = snapshot
        .ranged
        .by_account
        .get(&window)
        .into_iter()
        .flatten()
        .map(|(id, s)| (id.clone(), cut(s)))
        .collect();
    out.insert("by_account".into(), Value::Object(by_account));
    let mut value = Value::Object(out);
    let too_big = serde_json::to_vec(&value).is_ok_and(|b| b.len() > MAX_REPLY_BYTES);
    if too_big {
        if let Some(map) = value.as_object_mut() {
            map.remove("by_account");
        }
    }
    value
}

/// Logs each model with no price once per daemon run, so a missing price shows up beside the
/// cost views' own list of them without a line for every request.
pub struct UnpricedLog {
    logged: Mutex<BTreeSet<String>>,
    sink: Box<dyn Fn(&str) + Send + Sync>,
}

impl UnpricedLog {
    /// Writes to the daemon's own log, standard error.
    pub fn new() -> UnpricedLog {
        UnpricedLog::with_sink(Box::new(|line| eprintln!("{line}")))
    }

    pub fn with_sink(sink: Box<dyn Fn(&str) + Send + Sync>) -> UnpricedLog {
        UnpricedLog {
            logged: Mutex::new(BTreeSet::new()),
            sink,
        }
    }

    /// Logs the unpriced ids of every range in `snapshot` that weren't logged yet (an account's
    /// are among the overall ones).
    pub fn note(&self, snapshot: &Snapshot) {
        let ids = snapshot
            .ranged
            .overall
            .values()
            .flat_map(|s| s.unpriced.iter());
        let mut logged = self.logged.lock().unwrap_or_else(|e| e.into_inner());
        for id in ids {
            if logged.insert(id.clone()) {
                (self.sink)(&format!(
                    "notchling: no price for model {id}: its tokens are counted at $0; \
                     add a price to catalog.json"
                ));
            }
        }
    }
}

impl Default for UnpricedLog {
    fn default() -> UnpricedLog {
        UnpricedLog::new()
    }
}

/// `cost.summary`: answers with the published summaries and starts a scan if one is due.
pub fn call(
    service: &Arc<CostService>,
    unpriced: &UnpricedLog,
    args: &Value,
    now: f64,
) -> Result<Value, ApiError> {
    let window = range_of(args)?;
    let snapshot = service.request(now);
    unpriced.note(&snapshot);
    Ok(reply(&snapshot, window))
}
