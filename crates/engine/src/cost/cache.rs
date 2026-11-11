//! The tracker's own cache: totals, read offsets and the ids already counted, kept so a month of
//! transcripts (several GB) is read once and then only from where the last scan stopped.
//!
//! It is the [`CACHE_DOC`] document of `<data>/notchling.db` (it used to be the daemon's
//! `<data>/daemon/cost-cache.json`, which moves in once), never the Swift app's `cost-cache.json`,
//! and carries its own [`CURRENT`] version and the fingerprint of the prices its dollars were
//! counted at: a cache with another version or other prices, or one that can't be read, is dropped
//! and the history is read again (so a price fixed in the catalog reprices what was counted). Offsets, totals and counted ids are saved
//! together (one row), so a crash only means re-reading from the last save, never double
//! counting.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::store;

/// The document that holds the cache.
pub const CACHE_DOC: &str = "daemon/cost-cache";

/// The file the cache was kept in before the database.
pub fn old_cache_file(data: &Path) -> PathBuf {
    data.join("daemon").join("cost-cache.json")
}

/// Bump when the stored fields change; it forces a full re-read.
pub const CURRENT: u32 = 2;

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cache {
    pub version: u32,
    /// `Catalog::prices_hash` of the prices the dollars below were counted at.
    pub prices: String,
    pub files: BTreeMap<String, Mark>,
    /// Day ("2026-10-26", local time) to "account id, tab, model id" to dollars. Projects
    /// likewise.
    pub models: DayTable<f64>,
    pub projects: DayTable<f64>,
    /// Day to "account, channel" to dollars.
    pub channels: DayTable<f64>,
    /// Day to "account, model" to [input, output, cache read, cache write] token totals.
    pub tokens: DayTable<[f64; 4]>,
    /// Day to "account, channel" to all tokens, for channels whose cost is 0 (no known price).
    pub channel_tokens: DayTable<f64>,
    /// Day to the Claude model ids counted that day with no price (cost 0, tokens counted).
    pub unpriced: BTreeMap<String, BTreeSet<String>>,
    /// Day to the hashes of the message ids counted on that day.
    #[serde(with = "packed")]
    pub seen: BTreeMap<String, Vec<u64>>,
}

impl Cache {
    /// Nothing counted yet, at the prices with this fingerprint.
    pub fn empty(prices: &str) -> Cache {
        Cache {
            version: CURRENT,
            prices: prices.to_string(),
            files: BTreeMap::new(),
            models: BTreeMap::new(),
            projects: BTreeMap::new(),
            channels: BTreeMap::new(),
            tokens: BTreeMap::new(),
            channel_tokens: BTreeMap::new(),
            unpriced: BTreeMap::new(),
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
    /// when there is none, it can't be read, it is another version or it was counted at other
    /// `prices` (a `Catalog::prices_hash`).
    pub fn load(data: &Path, prices: &str) -> Cache {
        let _ = store::import_file(data, CACHE_DOC, &old_cache_file(data), parses);
        store::doc(data, CACHE_DOC)
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str::<Cache>(&text).ok())
            .filter(|c| c.version == CURRENT && c.prices == prices)
            .unwrap_or_else(|| Cache::empty(prices))
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
        self.models.retain(|d, _| !old(d));
        self.projects.retain(|d, _| !old(d));
        self.channels.retain(|d, _| !old(d));
        self.tokens.retain(|d, _| !old(d));
        self.channel_tokens.retain(|d, _| !old(d));
        self.unpriced.retain(|d, _| !old(d));
    }
}

/// Whether an old file's text is a cache of the current version (anything else isn't moved in).
fn parses(text: &str) -> bool {
    serde_json::from_str::<Cache>(text).is_ok_and(|c| c.version == CURRENT)
}
