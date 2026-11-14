use super::record::Fact;
use super::Tool;
use crate::store;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const DOC: &str = "versions/selfupdate-log";
pub const KEEP: usize = 200;
const DETAIL_MAX: usize = 300;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub seq: u64,
    pub at_ms: i64,
    pub tool: Tool,
    pub machine: String,
    pub action: String,
    pub ok: bool,
    pub detail: String,
}

impl Entry {
    pub fn fact(&self) -> Fact {
        Fact::SelfUpdate {
            tool: self.tool,
            machine: self.machine.clone(),
            action: self.action.clone(),
            ok: self.ok,
            detail: self.detail.clone(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Log {
    #[serde(default)]
    next: u64,
    #[serde(default)]
    recorded: u64,
    #[serde(default)]
    facts: Vec<Entry>,
}

fn parse(old: Option<String>) -> Log {
    old.and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn text(log: &Log) -> Result<Option<String>, String> {
    serde_json::to_string(log)
        .map(Some)
        .map_err(|e| e.to_string())
}

fn cut(detail: &str) -> String {
    detail
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(DETAIL_MAX)
        .collect()
}

pub fn append(
    data: &Path,
    at_ms: i64,
    tool: Tool,
    machine: &str,
    action: &str,
    ok: bool,
    detail: &str,
) -> Result<(), String> {
    store::update_doc(data, DOC, |old| {
        let mut log = parse(old);
        log.next += 1;
        let seq = log.next;
        log.facts.push(Entry {
            seq,
            at_ms,
            tool,
            machine: machine.to_string(),
            action: action.to_string(),
            ok,
            detail: cut(detail),
        });
        if log.facts.len() > KEEP {
            let extra = log.facts.len() - KEEP;
            log.facts.drain(..extra);
        }
        text(&log)
    })
    .map(|_| ())
}

pub fn unrecorded(data: &Path) -> Vec<Entry> {
    let log = parse(store::doc(data, DOC).ok().flatten());
    log.facts
        .into_iter()
        .filter(|e| e.seq > log.recorded)
        .collect()
}

pub fn mark_recorded(data: &Path, upto: u64) -> Result<(), String> {
    store::update_doc(data, DOC, |old| {
        let mut log = parse(old.clone());
        if upto <= log.recorded {
            return Ok(old);
        }
        log.recorded = upto;
        text(&log)
    })
    .map(|_| ())
}
