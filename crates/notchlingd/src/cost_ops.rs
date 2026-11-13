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

pub const MAX_ROWS: usize = 50;
pub const MAX_REPLY_BYTES: usize = 700_000;

#[derive(Clone, Debug)]
pub struct CostSetup {
    pub roots: Vec<Root>,
    pub cache_data: Option<PathBuf>,
    pub account_log: PathBuf,
    pub catalog: Arc<Catalog>,
}

impl CostSetup {
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

pub struct UnpricedLog {
    logged: Mutex<BTreeSet<String>>,
    sink: Box<dyn Fn(&str) + Send + Sync>,
}

impl UnpricedLog {
    pub fn new() -> UnpricedLog {
        UnpricedLog::with_sink(Box::new(|line| eprintln!("{line}")))
    }

    pub fn with_sink(sink: Box<dyn Fn(&str) + Send + Sync>) -> UnpricedLog {
        UnpricedLog {
            logged: Mutex::new(BTreeSet::new()),
            sink,
        }
    }

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
