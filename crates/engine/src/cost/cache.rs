//! The tracker's own cache: what was counted, read offsets and the ids already counted, kept so a
//! month of transcripts (several GB) is read once and then only from where the last scan stopped.
//!
//! It is the [`CACHE_DOC`] document of `<data>/notchling.db` (it used to be the daemon's
//! `<data>/daemon/cost-cache.json`, which moves in once), never the Swift app's `cost-cache.json`,
//! and carries its own [`CURRENT`] version. A cache of an unknown version, or one that can't be
//! read, is dropped and the history is read again; a version 2 cache is moved over in place (see
//! [`Cache::load`]), since the cache is the only record of a day whose transcripts are gone.
//!
//! What was counted is kept per day as [`Cell`]s (account, channel, model, project) with their
//! dollars and tokens, and each model's price is kept beside them (`priced_at`), so a price fixed
//! later reprices that model alone (the tracker's `reprice`) and nothing else is touched. Offsets,
//! cells and counted ids are saved together (one row), so a crash only means re-reading from the
//! last save, never double counting.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use super::summary::{account_of, name_of};
use super::tracker::{ANTIGRAVITY, CLAUDE_CODE, CODEX};
use crate::catalog::{Catalog, Price, Tool};
use crate::store;

/// The document that holds the cache.
pub const CACHE_DOC: &str = "daemon/cost-cache";

/// The file the cache was kept in before the database.
pub fn old_cache_file(data: &Path) -> PathBuf {
    data.join("daemon").join("cost-cache.json")
}

/// Bump when the stored fields change, and move the last version over in `load`: a full re-read
/// loses every day whose transcripts are gone.
pub const CURRENT: u32 = 3;

/// The version before cells, moved over by `load`.
pub const V2: u32 = 2;

/// Where reading stopped in one file. The rest is what a file's later lines depend on: Codex and
/// Antigravity name the model, folder and account once, before the lines that use them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    pub size: u64,
    pub offset: u64,
    pub model: Option<String>,
    pub project: Option<String>,
    pub account: Option<String>,
    /// Codex: `total_token_usage.total_tokens` of the last event counted, to skip a repeated one.
    pub total: Option<f64>,
}

type DayTable<V> = BTreeMap<String, BTreeMap<String, V>>;

/// Input, output, cache read, five-minute cache write and one-hour cache write tokens.
pub type Tokens = [f64; 5];

/// What one account spent on one model in one project through one channel on one day.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    /// Dollars, at the model's `priced_at` price.
    pub cost: f64,
    pub tokens: Tokens,
}

/// "account\tchannel\tmodel\tproject", the key of a day's cells.
pub fn cell_key(account: &str, channel: &str, model: &str, project: &str) -> String {
    format!("{account}\t{channel}\t{model}\t{project}")
}

/// A cell key's account, channel, model and project.
pub fn cell_parts(key: &str) -> (&str, &str, &str, &str) {
    let mut parts = key.splitn(4, '\t');
    let mut next = || parts.next().unwrap_or("");
    (next(), next(), next(), next())
}

/// The tool whose prices a channel's models have.
pub fn tool_of(channel: &str) -> Tool {
    match channel {
        CODEX => Tool::Codex,
        ANTIGRAVITY => Tool::Antigravity,
        _ => Tool::Claude,
    }
}

/// "tool\tmodel", the key of `priced_at`.
pub fn price_key(tool: Tool, model: &str) -> String {
    format!("{}\t{model}", tool.name())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cache {
    pub version: u32,
    pub files: BTreeMap<String, Mark>,
    /// Day ("2026-10-26", local time) to `cell_key` to what was counted.
    pub cells: DayTable<Cell>,
    /// `price_key` to the price its cells were counted at (`null`: the model had none, so its
    /// cells cost 0 and it is listed as unpriced). A model missing here was counted at a price
    /// nobody knows any more (a version 2 cache counted at other prices), and is priced again.
    pub priced_at: BTreeMap<String, Option<Price>>,
    /// Day to the hashes of the message ids counted on that day.
    #[serde(with = "packed")]
    pub seen: BTreeMap<String, Vec<u64>>,
}

impl Cache {
    /// Nothing counted yet.
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

/// Hashes packed as little-endian bytes, base64 in the file.
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
    /// The saved cache in `data`'s database (the old file moves in first, once), or an empty one
    /// when there is none, it can't be read or it is of an unknown version. A version 2 cache is
    /// moved over (`from_v2`), keeping every day it counted.
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

    /// Writes the cache as one row. An old file left beside the database (an unreadable one, or
    /// another version, which `load` ignored) is removed: the row replaces it.
    pub fn save(&self, data: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string(self).map_err(std::io::Error::other)?;
        store::put_doc(data, CACHE_DOC, &text).map_err(std::io::Error::other)?;
        let _ = std::fs::remove_file(old_cache_file(data));
        Ok(())
    }

    /// Every hash counted, for the dedupe set.
    pub fn seen_set(&self) -> HashSet<u64> {
        self.seen.values().flatten().copied().collect()
    }

    /// Forgets a day older than `cutoff` ("yyyy-MM-dd" strings compare in date order).
    pub fn prune_before(&mut self, cutoff: &str) {
        let old = |day: &String| day.as_str() < cutoff;
        self.seen.retain(|d, _| !old(d));
        self.cells.retain(|d, _| !old(d));
    }

    /// A version 2 cache, which kept a day's dollars by model, by project and by channel apart,
    /// moved over without reading a transcript: its offsets and counted ids stay, so nothing is
    /// read again. Each model's day is split over its account's projects and its tool's channels
    /// in proportion to their dollars (tokens: to the channels' tokens), which keeps every total,
    /// per model, per project and per channel, as it was. When the cache was counted at the
    /// catalog's prices (`prices_hash` is the same), each model's price is the catalog's; else it
    /// is left unknown and the tracker prices those days again.
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
                        // Version 2 kept cache writes as one number; they read as five-minute ones.
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

/// Each name's share of the values' sum, or an equal share when they sum to nothing.
fn shares(values: &BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    let sum: f64 = values.values().sum();
    let n = values.len().max(1) as f64;
    values
        .iter()
        .map(|(k, v)| (k.clone(), if sum > 0.0 { v / sum } else { 1.0 / n }))
        .collect()
}

/// The tool a version 2 model key was counted for, which it didn't keep: Antigravity's requests
/// are all on the account "agy", Claude's model ids start `claude-`, and the rest are Codex's.
fn v2_tool(account: &str, model: &str) -> Tool {
    if account == "agy" {
        Tool::Antigravity
    } else if model.starts_with("claude-") {
        Tool::Claude
    } else {
        Tool::Codex
    }
}

/// The channel a tool's requests are counted in when nothing says which.
fn main_channel(tool: Tool) -> &'static str {
    match tool {
        Tool::Claude => CLAUDE_CODE,
        Tool::Codex => CODEX,
        Tool::Antigravity => ANTIGRAVITY,
    }
}

/// The version 2 cache: dollars by model, project and channel apart, counted at the prices with
/// the fingerprint `prices` (`Catalog::prices_hash`).
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

/// Whether an old file's text is a cache this version reads (anything else isn't moved in).
fn parses(text: &str) -> bool {
    serde_json::from_str::<Cache>(text).is_ok_and(|c| c.version == CURRENT)
        || serde_json::from_str::<CacheV2>(text).is_ok_and(|c| c.version == V2)
}
