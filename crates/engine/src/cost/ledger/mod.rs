pub mod claude;
pub mod db;
pub mod guard;
pub mod ingest;
pub mod other;

use super::tracker::{Root, ZoneFn};
use crate::catalog::Catalog;
use crate::pages::clock::local_offset_secs;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const ROW_DAYS: i64 = 30;
pub const ROLLUP_DAYS: i64 = 365;
pub const FILE: &str = "usage.db";

pub const TOOL_CLAUDE: &str = "Claude Code";
pub const TOOL_CODEX: &str = "Codex";
pub const TOOL_ANTIGRAVITY: &str = "Antigravity";

#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub ts: f64,
    pub tool: String,
    pub session: String,
    pub agent: String,
    pub sub: bool,
    pub agent_type: Option<String>,
    pub model: String,
    pub effort: Option<String>,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub write_5m: u64,
    pub write_1h: u64,
    pub split: bool,
    pub cost: Option<f64>,
}

impl Call {
    pub fn writes(&self) -> u64 {
        self.write_5m + self.write_1h
    }

    pub fn context(&self) -> u64 {
        self.input + self.cache_read + self.write_5m + self.write_1h
    }

    pub fn tokens(&self) -> u64 {
        self.context() + self.output
    }

    pub fn stream(&self) -> (&str, &str, &str) {
        (&self.tool, &self.session, &self.agent)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rollup {
    pub day: String,
    pub tool: String,
    pub sub: bool,
    pub model: String,
    pub calls: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub write_5m: u64,
    pub write_1h: u64,
    pub cost: f64,
    pub unpriced: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub files: usize,
    pub left: usize,
    pub added: u64,
}

impl Progress {
    pub fn complete(&self) -> bool {
        self.left == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerError(pub String);

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<rusqlite::Error> for LedgerError {
    fn from(e: rusqlite::Error) -> LedgerError {
        LedgerError(e.to_string())
    }
}

impl From<std::io::Error> for LedgerError {
    fn from(e: std::io::Error) -> LedgerError {
        LedgerError(e.to_string())
    }
}

pub struct Ledger {
    pub(super) db: Connection,
    roots: Vec<Root>,
    catalog: Arc<Catalog>,
    zone: ZoneFn,
    guard_dir: Option<PathBuf>,
}

impl Ledger {
    pub fn open(
        path: &Path,
        roots: Vec<Root>,
        catalog: Arc<Catalog>,
    ) -> Result<Ledger, LedgerError> {
        Ok(Ledger::with(db::open_file(path)?, roots, catalog))
    }

    pub fn in_memory(roots: Vec<Root>, catalog: Arc<Catalog>) -> Result<Ledger, LedgerError> {
        Ok(Ledger::with(db::open_memory()?, roots, catalog))
    }

    fn with(db: Connection, roots: Vec<Root>, catalog: Arc<Catalog>) -> Ledger {
        Ledger {
            db,
            roots,
            catalog,
            zone: local_offset_secs,
            guard_dir: None,
        }
    }

    pub fn with_guard_dir(mut self, dir: PathBuf) -> Ledger {
        self.guard_dir = Some(dir);
        self
    }

    pub fn take_guard(&self) -> usize {
        let Some(taken) = self
            .guard_dir
            .as_deref()
            .and_then(crate::guard::spool::take)
        else {
            return 0;
        };
        let added = taken.rows.len();
        if guard::insert_all(&self.db, &taken.rows).is_ok() {
            taken.done();
            return added;
        }
        0
    }

    pub fn guard_since(&self, from: f64, to: f64, days: u32) -> guard::GuardReport {
        guard::summary(&self.db, from, to, days)
    }

    pub fn with_zone(mut self, zone: ZoneFn) -> Ledger {
        self.zone = zone;
        self
    }

    pub fn day_number(&self, ts: f64) -> i64 {
        (ts.floor() as i64 + (self.zone)(ts)).div_euclid(86_400)
    }

    pub fn refresh(&mut self, now: f64, budget: Option<Duration>) -> Progress {
        ingest::refresh(self, now, budget)
    }

    pub fn calls_since(&self, from: f64) -> Vec<Call> {
        db::calls_since(&self.db, from)
    }

    pub fn rollups_since(&self, day: &str) -> Vec<Rollup> {
        db::rollups_since(&self.db, day)
    }

    pub fn call_count(&self) -> u64 {
        db::call_count(&self.db)
    }

    pub fn scanned(&self) -> Option<f64> {
        db::meta_f64(&self.db, "scanned")
    }

    pub fn prune(&mut self, now: f64) {
        let rows = now - (ROW_DAYS * 86_400) as f64;
        let day = super::summary::day_key(self.day_number(now) - ROLLUP_DAYS);
        db::prune(&self.db, rows, &day);
    }

    pub(super) fn roots(&self) -> &[Root] {
        &self.roots
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }
}
