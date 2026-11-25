use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use super::summary::{account_of, name_of};
use super::tracker::{ANTIGRAVITY, CLAUDE_CODE, CODEX};
use crate::catalog::{Catalog, Price, Tool};
use crate::store;

pub const CACHE_DOC: &str = "daemon/cost-cache";

pub fn old_cache_file(data: &Path) -> PathBuf {
    data.join("daemon").join("cost-cache.json")
}

pub const CURRENT: u32 = 3;

pub const V2: u32 = 2;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    pub size: u64,
    pub offset: u64,
    pub model: Option<String>,
    pub project: Option<String>,
    pub account: Option<String>,
    pub total: Option<f64>,
}

type DayTable<V> = BTreeMap<String, BTreeMap<String, V>>;

pub type Tokens = [f64; 5];

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub cost: f64,
    pub tokens: Tokens,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub moved: bool,
}

pub fn cell_key(account: &str, channel: &str, model: &str, project: &str) -> String {
    format!("{account}\t{channel}\t{model}\t{project}")
}

pub fn cell_parts(key: &str) -> (&str, &str, &str, &str) {
    let mut parts = key.splitn(4, '\t');
    let mut next = || parts.next().unwrap_or("");
    (next(), next(), next(), next())
}

pub fn tool_of(channel: &str) -> Tool {
    match channel {
        CODEX => Tool::Codex,
        ANTIGRAVITY => Tool::Antigravity,
        _ => Tool::Claude,
    }
}

pub fn price_key(tool: Tool, model: &str) -> String {
    format!("{}\t{model}", tool.name())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cache {
    pub version: u32,
    pub files: BTreeMap<String, Mark>,
    pub cells: DayTable<Cell>,
    pub priced_at: BTreeMap<String, Option<Price>>,
    #[serde(with = "packed")]
    pub seen: BTreeMap<String, Vec<u64>>,
}

impl Cache {
    pub fn empty() -> Cache {
        Cache {
            version: CURRENT,
            files: BTreeMap::new(),
            cells: BTreeMap::new(),
            priced_at: BTreeMap::new(),
            seen: BTreeMap::new(),
        }
    }
}

mod packed {
    use super::*;

    pub fn serialize<S: Serializer>(
        seen: &BTreeMap<String, Vec<u64>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let text: BTreeMap<&String, String> = seen
            .iter()
            .map(|(day, hashes)| {
                let bytes: Vec<u8> = hashes.iter().flat_map(|h| h.to_le_bytes()).collect();
                (day, STANDARD.encode(bytes))
            })
            .collect();
        text.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<String, Vec<u64>>, D::Error> {
        let text = BTreeMap::<String, String>::deserialize(d)?;
        text.into_iter()
            .map(|(day, packed)| {
                let bytes = STANDARD.decode(packed).map_err(serde::de::Error::custom)?;
                let hashes = bytes
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| u64::from_le_bytes(*c))
                    .collect();
                Ok((day, hashes))
            })
            .collect()
    }
}

impl Cache {
    pub fn load(data: &Path, catalog: &Catalog) -> Cache {
        let _ = store::import_file(data, CACHE_DOC, &old_cache_file(data), parses);
        let Some(text) = store::doc(data, CACHE_DOC).ok().flatten() else {
            return Cache::empty();
        };
        if let Some(cache) = serde_json::from_str::<Cache>(&text)
            .ok()
            .filter(|c| c.version == CURRENT)
        {
            return cache;
        }
        serde_json::from_str::<CacheV2>(&text)
            .ok()
            .filter(|c| c.version == V2)
            .map(|old| Cache::from_v2(old, catalog))
            .unwrap_or_else(Cache::empty)
    }

    pub fn save(&self, data: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string(self).map_err(std::io::Error::other)?;
        store::put_doc(data, CACHE_DOC, &text).map_err(std::io::Error::other)?;
        let _ = std::fs::remove_file(old_cache_file(data));
        Ok(())
    }

    pub fn seen_set(&self) -> HashSet<u64> {
        self.seen.values().flatten().copied().collect()
    }

    pub fn prune_before(&mut self, cutoff: &str) {
        let old = |day: &String| day.as_str() < cutoff;
        self.seen.retain(|d, _| !old(d));
        self.cells.retain(|d, _| !old(d));
    }

    fn from_v2(old: CacheV2, catalog: &Catalog) -> Cache {
        let known = old.prices == catalog.prices_hash();
        let mut cache = Cache::empty();
        cache.files = old.files;
        cache.seen = old.seen;
        let days: BTreeSet<&String> = old
            .models
            .keys()
            .chain(old.tokens.keys())
            .chain(old.projects.keys())
            .chain(old.channels.keys())
            .collect();
        let nothing = BTreeMap::new();
        for day in days {
            let day_of = |t: &DayTable<f64>| t.get(day).unwrap_or(&nothing).clone();
            let projects = day_of(&old.projects);
            let channels = day_of(&old.channels);
            let channel_tokens = day_of(&old.channel_tokens);
            let mut used: BTreeMap<String, (f64, [f64; 4])> = BTreeMap::new();
            for (k, cost) in day_of(&old.models) {
                used.entry(k).or_default().0 += cost;
            }
            for (k, t) in old.tokens.get(day).into_iter().flatten() {
                used.entry(k.clone()).or_default().1 = *t;
            }
            let cells = cache.cells.entry(day.clone()).or_default();
            for (k, (cost, t)) in used {
                let (account, model) = (account_of(&k), name_of(&k));
                let tool = v2_tool(account, model);
                let mine = |table: &BTreeMap<String, f64>, of_tool: bool| {
                    table
                        .iter()
                        .filter(|(k, _)| account_of(k) == account)
                        .filter(|(k, _)| !of_tool || tool_of(name_of(k)) == tool)
                        .map(|(k, v)| (name_of(k).to_string(), *v))
                        .collect::<BTreeMap<String, f64>>()
                };
                let mut chans = mine(&channels, true);
                let chan_tokens = mine(&channel_tokens, true);
                for ch in chan_tokens.keys() {
                    chans.entry(ch.clone()).or_insert(0.0);
                }
                if chans.is_empty() {
                    chans.insert(main_channel(tool).to_string(), 0.0);
                }
                let mut projs = mine(&projects, false);
                if projs.is_empty() {
                    projs.insert(main_channel(tool).to_string(), 0.0);
                }
                let cost_share = shares(&chans);
                let token_share = if chan_tokens.values().sum::<f64>() > 0.0 {
                    let by_tokens = chans
                        .keys()
                        .map(|c| (c.clone(), chan_tokens.get(c).copied().unwrap_or(0.0)))
                        .collect();
                    shares(&by_tokens)
                } else {
                    cost_share.clone()
                };
                let proj_share = shares(&projs);
                for (ch, cs) in &cost_share {
                    let ts = token_share.get(ch).copied().unwrap_or(0.0);
                    for (p, ps) in &proj_share {
                        let cell = cells.entry(cell_key(account, ch, model, p)).or_default();
                        cell.cost += cost * cs * ps;
                        cell.moved = true;
                        for (sum, n) in cell.tokens.iter_mut().zip(t) {
                            *sum += n * ts * ps;
                        }
                    }
                }
                if known {
                    cache
                        .priced_at
                        .insert(price_key(tool, model), catalog.price(tool, model));
                }
            }
        }
        cache.cells.retain(|_, cells| !cells.is_empty());
        cache
    }
}

fn shares(values: &BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    let sum: f64 = values.values().sum();
    let n = values.len().max(1) as f64;
    values
        .iter()
        .map(|(k, v)| (k.clone(), if sum > 0.0 { v / sum } else { 1.0 / n }))
        .collect()
}

fn v2_tool(account: &str, model: &str) -> Tool {
    if account == "agy" {
        Tool::Antigravity
    } else if model.starts_with("claude-") {
        Tool::Claude
    } else {
        Tool::Codex
    }
}

fn main_channel(tool: Tool) -> &'static str {
    match tool {
        Tool::Claude => CLAUDE_CODE,
        Tool::Codex => CODEX,
        Tool::Antigravity => ANTIGRAVITY,
    }
}

#[derive(Deserialize)]
struct CacheV2 {
    version: u32,
    prices: String,
    files: BTreeMap<String, Mark>,
    models: DayTable<f64>,
    projects: DayTable<f64>,
    channels: DayTable<f64>,
    tokens: DayTable<[f64; 4]>,
    channel_tokens: DayTable<f64>,
    #[serde(with = "packed")]
    seen: BTreeMap<String, Vec<u64>>,
}

fn parses(text: &str) -> bool {
    serde_json::from_str::<Cache>(text).is_ok_and(|c| c.version == CURRENT)
        || serde_json::from_str::<CacheV2>(text).is_ok_and(|c| c.version == V2)
}
