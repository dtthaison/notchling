//! The tracker's own cache: totals, read offsets and the ids already counted, kept so a month of
//! transcripts (several GB) is read once and then only from where the last scan stopped.
//!
//! It lives in the daemon's folder (`<data>/daemon/cost-cache.json`), never in the Swift app's
//! `cost-cache.json`, and carries its own [`CURRENT`] version: a cache with another version, or
//! one that can't be read, is dropped and the history is read again. Offsets, totals and counted
//! ids are saved together, so a crash only means re-reading from the last save, never double
//! counting.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// Bump when the stored fields change; it forces a full re-read.
pub const CURRENT: u32 = 1;

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
    /// Day to the hashes of the message ids counted on that day.
    #[serde(with = "packed")]
    pub seen: BTreeMap<String, Vec<u64>>,
}

impl Default for Cache {
    fn default() -> Cache {
        Cache {
            version: CURRENT,
            files: BTreeMap::new(),
            models: BTreeMap::new(),
            projects: BTreeMap::new(),
            channels: BTreeMap::new(),
            tokens: BTreeMap::new(),
            channel_tokens: BTreeMap::new(),
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
    /// The saved cache, or an empty one when there is none, it can't be read or it is another
    /// version.
    pub fn load(file: &Path) -> Cache {
        std::fs::read(file)
            .ok()
            .and_then(|data| serde_json::from_slice::<Cache>(&data).ok())
            .filter(|c| c.version == CURRENT)
            .unwrap_or_default()
    }

    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            notchling_platform::files::ensure_private_dir(dir).map_err(std::io::Error::other)?;
        }
        let data = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        notchling_platform::files::write_private_no_follow(file, &data)
            .map_err(std::io::Error::other)
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
    }
}
