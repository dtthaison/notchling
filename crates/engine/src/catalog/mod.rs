//! The model catalog: which models each tool offers, their efforts, context windows and prices,
//! as data (`data/catalog.json`) instead of tables spread over the code.
//!
//! The data file says what is allowed. A user's override file (`Catalog::load`) can fix a price, a
//! window or a name, but never adds a model to the allowlist: only the built-in lists and what the
//! tool itself reports live can.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cost::prices::display_name;

pub mod claude_live;
pub mod live_cache;

const BUILT_IN: &str = include_str!("../../data/catalog.json");

/// The one Claude model with no effort levels.
const NO_EFFORT: &str = "claude-haiku-4-5";

/// A tool whose models the catalog knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Claude,
    Codex,
    Antigravity,
}

impl Tool {
    const ALL: [Tool; 3] = [Tool::Claude, Tool::Codex, Tool::Antigravity];

    pub fn parse(name: &str) -> Option<Tool> {
        match name {
            "claude" => Some(Tool::Claude),
            "codex" => Some(Tool::Codex),
            "antigravity" => Some(Tool::Antigravity),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
            Tool::Antigravity => "antigravity",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Dollars per million tokens. Cache writes are not listed: they are 1.25x input for five minutes
/// and 2x for an hour.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub input: f64,
    pub output: f64,
    #[serde(alias = "cache_read")]
    pub cache_read: f64,
    /// The dearer tier a long prompt costs (Haiku 5.5's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub over: Option<Over>,
}

/// The prices once a prompt (input, cache read and cache write) is over `tokens`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Over {
    pub tokens: f64,
    pub input: f64,
    pub output: f64,
    #[serde(alias = "cache_read")]
    pub cache_read: f64,
}

/// Whether a model is one the pickers lead with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Current,
    Older,
}

/// One model as the tool itself lists it. Every field but the id may be missing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveModel {
    pub id: String,
    pub name: Option<String>,
    pub context_window: Option<u64>,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

/// A model with everything the catalog knows about it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub tier: Tier,
    pub context_window: Option<u64>,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    pub price: Option<Price>,
    /// The price and window are those of the newest known model of its family, since this one is
    /// newer than any the catalog has heard of.
    pub borrowed: bool,
}

/// A price and what it is for: a Claude id prefix (`match`) or a whole Codex/Antigravity id (`id`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PriceEntry {
    #[serde(rename = "match", alias = "id")]
    key: String,
    #[serde(flatten)]
    price: Price,
}

#[derive(Clone, Debug, Default)]
struct ToolData {
    prices: Vec<PriceEntry>,
    windows: BTreeMap<String, u64>,
    names: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    current: Vec<String>,
    older: Vec<String>,
    efforts: Vec<String>,
    tools: [ToolData; 3],
}

/// "claude-opus-5-5[1m]", "claude-haiku-4-5-20251001" and "gpt-5.1-2026-01-01" are
/// "claude-opus-5-5", "claude-haiku-4-5" and "gpt-5.1".
pub fn model_key(model: &str) -> String {
    let id = model.split('[').next().unwrap_or(model);
    let b = id.as_bytes();
    let n = b.len();
    let digits = |s: &[u8]| s.iter().all(u8::is_ascii_digit);
    if n >= 9 && b[n - 9] == b'-' && digits(&b[n - 8..]) {
        return id[..n - 9].to_string();
    }
    if n >= 11
        && b[n - 11] == b'-'
        && digits(&b[n - 10..n - 6])
        && b[n - 6] == b'-'
        && digits(&b[n - 5..n - 3])
        && b[n - 3] == b'-'
        && digits(&b[n - 2..])
    {
        return id[..n - 11].to_string();
    }
    id.to_string()
}

/// "claude-fable-5-2" is family "fable", version [5, 2]. Ids that don't start `claude-<name>` have
/// no family.
fn family_version(id: &str) -> Option<(String, Vec<u64>)> {
    let mut parts = id.strip_prefix("claude-")?.split('-');
    let family = parts
        .next()
        .filter(|f| !f.starts_with(|c: char| c.is_ascii_digit()))?;
    if family.is_empty() {
        return None;
    }
    let version = parts.map_while(|p| p.parse::<u64>().ok()).collect();
    Some((family.to_string(), version))
}

/// The newest of `ids` in `family`.
fn newest_known<'a>(
    ids: impl Iterator<Item = &'a String>,
    family: &str,
) -> Option<(&'a str, Vec<u64>)> {
    let mut best: Option<(&str, Vec<u64>)> = None;
    for id in ids {
        let Some((f, v)) = family_version(id) else {
            continue;
        };
        if f == family && best.as_ref().is_none_or(|(_, bv)| v > *bv) {
            best = Some((id, v));
        }
    }
    best
}

impl Catalog {
    /// What ships in the app.
    pub fn built_in() -> Catalog {
        let mut catalog = Catalog::default();
        if let Ok(root) = serde_json::from_str::<Value>(BUILT_IN) {
            catalog.apply(&root, true);
        }
        catalog.finish();
        catalog
    }

    /// The built-in catalog with the user's override file laid over its prices, windows and names.
    /// A missing or unreadable file, or a part of it that doesn't parse, changes nothing.
    pub fn load(override_file: &Path) -> Catalog {
        let mut catalog = Catalog::built_in();
        let root = std::fs::read_to_string(override_file)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok());
        if let Some(root) = root {
            catalog.apply(&root, false);
            catalog.finish();
        }
        catalog
    }

    /// Reads `root` entry by entry, so one bad entry costs only itself. The model lists and efforts
    /// come only from the built-in file.
    fn apply(&mut self, root: &Value, built_in: bool) {
        let Some(root) = root.as_object() else {
            return;
        };
        for tool in Tool::ALL {
            let Some(section) = root.get(tool.name()).and_then(Value::as_object) else {
                continue;
            };
            let data = &mut self.tools[tool.index()];
            for entry in section
                .get("prices")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Ok(mut entry) = serde_json::from_value::<PriceEntry>(entry.clone()) else {
                    continue;
                };
                entry.key = entry.key.trim().to_lowercase();
                if entry.key.is_empty() {
                    continue;
                }
                match data.prices.iter_mut().find(|p| p.key == entry.key) {
                    Some(slot) => *slot = entry,
                    None => data.prices.push(entry),
                }
            }
            for (id, window) in section
                .get("windows")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                if let Some(window) = window.as_u64() {
                    data.windows.insert(id.to_lowercase(), window);
                }
            }
            for (id, name) in section
                .get("names")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                if let Some(name) = name.as_str() {
                    data.names.insert(id.to_lowercase(), name.to_string());
                }
            }
            if built_in && tool == Tool::Claude {
                let strings = |key: &str| -> Vec<String> {
                    section
                        .get(key)
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                };
                self.current = strings("current");
                self.older = strings("older");
                self.efforts = strings("efforts");
            }
        }
    }

    /// Claude's prices match by prefix and the first match wins, so the longest prefix goes first
    /// (an override's new entry would otherwise land behind the bare family's). Stable, so equal
    /// lengths keep the file's order.
    fn finish(&mut self) {
        self.tools[Tool::Claude.index()]
            .prices
            .sort_by_key(|p| std::cmp::Reverse(p.key.len()));
    }

    fn known(&self) -> impl Iterator<Item = &String> {
        self.current.iter().chain(&self.older)
    }

    /// For a Claude id newer than every known id of its family, the newest known one: the catalog
    /// prices and sizes it as that one, so cost and the picker agree.
    fn borrow_source(&self, key: &str) -> Option<&str> {
        let (family, version) = family_version(key)?;
        let (known, known_version) = newest_known(self.known(), &family)?;
        (version > known_version).then_some(known)
    }

    /// Dollars per million tokens, or `None` for a model with no price (shown as unpriced, never $0).
    pub fn price(&self, tool: Tool, model: &str) -> Option<Price> {
        let key = model_key(model).to_lowercase();
        if tool == Tool::Claude {
            return self.claude_price(&key).0;
        }
        let prices = &self.tools[tool.index()].prices;
        prices.iter().find(|p| p.key == key).map(|p| p.price)
    }

    /// A Claude id's price and whether it is another model's. An id newer than every known one
    /// takes the newest known id's price, unless an entry made for it (not a family's general one,
    /// which the known id matches too) says otherwise.
    fn claude_price(&self, key: &str) -> (Option<Price>, bool) {
        let prices = &self.tools[Tool::Claude.index()].prices;
        let bare = |id: &str| id.strip_prefix("claude-").unwrap_or(id).to_string();
        // An entry is for an id when it is the id or a leading run of its `-` segments, so
        // `fable-5-1` is not for `fable-5-10`.
        let find = |id: &str| {
            let id = bare(id);
            prices
                .iter()
                .find(|p| id == p.key || id.starts_with(&format!("{}-", p.key)))
        };
        let Some(source) = self.borrow_source(key) else {
            return (find(key).map(|p| p.price), false);
        };
        let source_bare = bare(source);
        match find(key) {
            Some(own) if !source_bare.starts_with(&own.key) => (Some(own.price), false),
            _ => (find(source).map(|p| p.price), true),
        }
    }

    /// True when `price` (and `context_window`) for this Claude id are another model's.
    pub fn price_borrowed(&self, tool: Tool, model: &str) -> bool {
        tool == Tool::Claude && self.claude_price(&model_key(model).to_lowercase()).1
    }

    pub fn context_window(&self, tool: Tool, model: &str) -> Option<u64> {
        let key = model_key(model).to_lowercase();
        let windows = &self.tools[tool.index()].windows;
        let own = windows.get(&key).copied();
        if tool != Tool::Claude || own.is_some() {
            return own;
        }
        let source = self.borrow_source(&key)?;
        windows.get(source).copied()
    }

    /// Every model id a session may be started with. Claude's are the built-in lists plus what the
    /// tool lists live (bare and as listed, as a `[1m]` id is the bare model); the other tools' are
    /// only what they list. Check membership with `is_allowed`, which sees through a `[1m]` or a
    /// date suffix.
    pub fn allowed(&self, tool: Tool, live: &[LiveModel]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut add = |id: String| {
            if !out.contains(&id) {
                out.push(id);
            }
        };
        if tool == Tool::Claude {
            self.known().for_each(|id| add(id.clone()));
            for l in live {
                add(model_key(&l.id));
            }
        }
        for l in live {
            add(l.id.clone());
        }
        out
    }

    /// Whether `id` may start a session: it, or its bare form (no `[1m]`, no date), is one of
    /// `allowed`, whose ids are compared bare too.
    pub fn is_allowed(&self, tool: Tool, live: &[LiveModel], id: &str) -> bool {
        let bare = model_key(id);
        self.allowed(tool, live)
            .iter()
            .any(|a| a == id || model_key(a) == bare)
    }

    /// The models to offer, current ones first.
    pub fn models(&self, tool: Tool, live: &[LiveModel]) -> Vec<ModelInfo> {
        if tool == Tool::Claude {
            return self.claude_models(live);
        }
        live.iter()
            .map(|l| ModelInfo {
                id: l.id.clone(),
                name: self.name(tool, &l.id, l),
                tier: Tier::Current,
                context_window: l
                    .context_window
                    .or_else(|| self.context_window(tool, &l.id)),
                efforts: l.efforts.clone(),
                default_effort: l.default_effort.clone(),
                price: self.price(tool, &l.id),
                borrowed: false,
            })
            .collect()
    }

    /// The override's name, else the tool's own, else one made from the id.
    fn name(&self, tool: Tool, id: &str, live: &LiveModel) -> String {
        self.tools[tool.index()]
            .names
            .get(&id.to_lowercase())
            .cloned()
            .or_else(|| live.name.clone())
            .unwrap_or_else(|| display_name(id))
    }

    fn claude_models(&self, live: &[LiveModel]) -> Vec<ModelInfo> {
        let mut current = self.current.clone();
        let mut older = self.older.clone();
        let mut by_id: HashMap<String, &LiveModel> = HashMap::new();
        for l in live {
            let id = model_key(&l.id);
            if by_id.contains_key(&id) {
                continue;
            }
            by_id.insert(id.clone(), l);
            if current.contains(&id) || older.contains(&id) {
                continue;
            }
            // A model newer than all its family's known ones takes the family's place in current;
            // the model it displaces becomes the newest of the older ones.
            let place = family_version(&id)
                .filter(|(family, version)| {
                    newest_known(current.iter().chain(&older), family)
                        .is_some_and(|(_, known)| *version > known)
                })
                .and_then(|(family, _)| {
                    current
                        .iter()
                        .position(|c| family_version(c).is_some_and(|(f, _)| f == family))
                });
            match place {
                Some(at) => {
                    let displaced = std::mem::replace(&mut current[at], id);
                    older.insert(0, displaced);
                }
                None => older.push(id),
            }
        }
        let info = |id: &String, tier: Tier| {
            let l = by_id.get(id.as_str());
            let name = self.tools[Tool::Claude.index()]
                .names
                .get(&id.to_lowercase())
                .cloned()
                .or_else(|| l.and_then(|l| l.name.clone()))
                .unwrap_or_else(|| display_name(id));
            ModelInfo {
                id: id.clone(),
                name,
                tier,
                context_window: self
                    .context_window(Tool::Claude, id)
                    .or_else(|| l.and_then(|l| l.context_window)),
                efforts: if id == NO_EFFORT {
                    Vec::new()
                } else {
                    self.efforts.clone()
                },
                default_effort: l.and_then(|l| l.default_effort.clone()),
                price: self.price(Tool::Claude, id),
                borrowed: self.price_borrowed(Tool::Claude, id),
            }
        };
        let now = current.iter().map(|id| info(id, Tier::Current));
        now.chain(older.iter().map(|id| info(id, Tier::Older)))
            .collect()
    }

    /// A stable fingerprint of every price after the override is merged, so a change to a price
    /// shows (and costs counted at the old one can be counted again). Windows and names don't count.
    pub fn prices_hash(&self) -> String {
        let lists: Vec<&Vec<PriceEntry>> = self.tools.iter().map(|t| &t.prices).collect();
        let canonical = serde_json::to_string(&lists).unwrap_or_default();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in canonical.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{hash:016x}")
    }
}

#[cfg(test)]
mod claude_live_tests;
#[cfg(test)]
mod live_cache_tests;
#[cfg(test)]
mod tests;
