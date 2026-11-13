use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cost::prices::display_name;

pub mod claude_code;
pub mod claude_live;
pub mod live_cache;

const BUILT_IN: &str = include_str!("../../data/catalog.json");

const NO_EFFORT: &str = "claude-haiku-4-5";

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

    pub fn name(self) -> &'static str {
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub input: f64,
    pub output: f64,
    #[serde(alias = "cache_read")]
    pub cache_read: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub over: Option<Over>,
}

impl Price {
    fn sane(&self) -> bool {
        let ok = |x: f64| x.is_finite() && x >= 0.0;
        ok(self.input)
            && ok(self.output)
            && ok(self.cache_read)
            && self
                .over
                .is_none_or(|o| ok(o.tokens) && ok(o.input) && ok(o.output) && ok(o.cache_read))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Over {
    pub tokens: f64,
    pub input: f64,
    pub output: f64,
    #[serde(alias = "cache_read")]
    pub cache_read: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Current,
    Older,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LiveModel {
    pub id: String,
    pub name: Option<String>,
    pub context_window: Option<u64>,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

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
    pub borrowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub borrowed_from: Option<String>,
}

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

pub(crate) fn startable_shape(id: &str) -> bool {
    let name = id.strip_suffix("[1m]").unwrap_or(id);
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

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

fn plain_release(id: &str) -> bool {
    let rest: Vec<&str> = id
        .strip_prefix("claude-")
        .map(|r| r.split('-').skip(1).collect())
        .unwrap_or_default();
    !rest.is_empty() && rest.iter().all(|p| p.parse::<u64>().is_ok())
}

fn plain_name(name: &str) -> String {
    name.strip_prefix("Claude ").unwrap_or(name).to_string()
}

fn claude_usable(live: &[LiveModel]) -> impl Iterator<Item = &LiveModel> {
    live.iter()
        .filter(|l| !l.id.is_empty() && startable_shape(&l.id))
}

fn newest_known<'a>(
    ids: impl Iterator<Item = &'a String>,
    family: &str,
) -> Option<(&'a str, Vec<u64>)> {
    let mut best: Option<(&str, Vec<u64>)> = None;
    for id in ids.filter(|id| plain_release(id)) {
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
    pub fn built_in() -> Catalog {
        let mut catalog = Catalog::default();
        if let Ok(root) = serde_json::from_str::<Value>(BUILT_IN) {
            catalog.apply(&root, true);
        }
        catalog.finish();
        catalog
    }

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
                if tool == Tool::Claude {
                    if let Some(bare) = entry.key.strip_prefix("claude-") {
                        entry.key = bare.to_string();
                    }
                }
                if entry.key.is_empty() || !entry.price.sane() {
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

    fn finish(&mut self) {
        self.tools[Tool::Claude.index()]
            .prices
            .sort_by_key(|p| std::cmp::Reverse(p.key.len()));
    }

    fn known(&self) -> impl Iterator<Item = &String> {
        self.current.iter().chain(&self.older)
    }

    fn borrow_source(&self, key: &str) -> Option<&str> {
        let (family, version) = family_version(key)?;
        let (known, known_version) = newest_known(self.known(), &family)?;
        (version > known_version).then_some(known)
    }

    pub fn price(&self, tool: Tool, model: &str) -> Option<Price> {
        let key = model_key(model).to_lowercase();
        if tool == Tool::Claude {
            return self.claude_price(&key).0;
        }
        let prices = &self.tools[tool.index()].prices;
        prices.iter().find(|p| p.key == key).map(|p| p.price)
    }

    fn claude_price(&self, key: &str) -> (Option<Price>, bool) {
        let prices = &self.tools[Tool::Claude.index()].prices;
        let bare = |id: &str| id.strip_prefix("claude-").unwrap_or(id).to_string();
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

    pub fn allowed(&self, tool: Tool, live: &[LiveModel]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut add = |id: String| {
            if !out.contains(&id) {
                out.push(id);
            }
        };
        if tool == Tool::Claude {
            self.known().for_each(|id| add(id.clone()));
            for l in claude_usable(live) {
                add(model_key(&l.id));
                add(l.id.clone());
            }
            return out;
        }
        for l in live {
            add(l.id.clone());
        }
        out
    }

    pub fn is_allowed(&self, tool: Tool, live: &[LiveModel], id: &str) -> bool {
        if tool != Tool::Claude {
            return self.allowed(tool, live).iter().any(|a| a == id);
        }
        if !startable_shape(id) {
            return false;
        }
        let bare = model_key(id);
        self.allowed(tool, live)
            .iter()
            .any(|a| a == id || model_key(a) == bare)
    }

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
                borrowed_from: None,
            })
            .collect()
    }

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
        for l in claude_usable(live) {
            let id = model_key(&l.id);
            if by_id.contains_key(&id) {
                continue;
            }
            by_id.insert(id.clone(), l);
            if current.contains(&id) || older.contains(&id) {
                continue;
            }
            let place = family_version(&id)
                .filter(|_| plain_release(&id))
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
                .map(|n| plain_name(&n))
                .unwrap_or_else(|| display_name(id));
            let borrowed = self.price_borrowed(Tool::Claude, id);
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
                borrowed,
                borrowed_from: borrowed
                    .then(|| self.borrow_source(&model_key(id).to_lowercase()))
                    .flatten()
                    .map(str::to_owned),
            }
        };
        let now = current.iter().map(|id| info(id, Tier::Current));
        now.chain(older.iter().map(|id| info(id, Tier::Older)))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn with_current(mut self, id: &str) -> Catalog {
        self.current.push(id.to_string());
        self
    }

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
mod claude_code_tests;
#[cfg(test)]
mod claude_live_tests;
#[cfg(test)]
mod live_cache_tests;
#[cfg(test)]
mod tests;
