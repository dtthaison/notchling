use crate::catalog::Tool;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub const VERSION: u64 = 1;
pub const KEY_PRESET: &str = "costPreset";
pub const KEY_PICKS: &str = "costPicks";
pub const KEY_TOOLS: [&str; 3] = ["costClaude", "costCodex", "costAgy"];
pub const APP_KEY: &str = "costProfileApp";

pub const CLAUDE_FAMILIES: [&str; 4] = ["haiku", "sonnet", "opus", "fable"];
pub const CLAUDE_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Read,
    Code,
    Fix,
    Judge,
    Hardest,
    Review,
    Image,
}

impl Role {
    pub const ALL: [Role; 7] = [
        Role::Read,
        Role::Code,
        Role::Fix,
        Role::Judge,
        Role::Hardest,
        Role::Review,
        Role::Image,
    ];

    pub const WORK: [Role; 5] = [
        Role::Read,
        Role::Code,
        Role::Fix,
        Role::Judge,
        Role::Hardest,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Role::Read => "read",
            Role::Code => "code",
            Role::Fix => "fix",
            Role::Judge => "judge",
            Role::Hardest => "hardest",
            Role::Review => "review",
            Role::Image => "image",
        }
    }

    pub fn parse(name: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.name() == name)
    }
}

pub fn tool_key(tool: Tool) -> &'static str {
    match tool {
        Tool::Claude => "claude",
        Tool::Codex => "codex",
        Tool::Antigravity => "agy",
    }
}

pub fn parse_tool(name: &str) -> Option<Tool> {
    Tool::parse(name)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub model: Option<String>,
    pub effort: String,
}

impl Cell {
    pub fn new(model: &str, effort: &str) -> Cell {
        Cell {
            model: Some(model.to_string()),
            effort: effort.to_string(),
        }
    }

    pub fn parse(tool: Tool, text: &str) -> Option<Cell> {
        let mut words = text.split_whitespace();
        let (model, effort) = (words.next()?, words.next()?);
        if words.next().is_some() {
            return None;
        }
        let plain = |s: &str| {
            !s.is_empty()
                && s.chars().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_')
                })
        };
        if !plain(effort) {
            return None;
        }
        let model = match (tool, model) {
            (Tool::Claude, m) => {
                if !CLAUDE_FAMILIES.contains(&m) || !CLAUDE_EFFORTS.contains(&effort) {
                    return None;
                }
                Some(m.to_string())
            }
            (_, "-") => None,
            (_, m) if plain(m) => Some(m.to_string()),
            _ => return None,
        };
        Some(Cell {
            model,
            effort: effort.to_string(),
        })
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}",
            self.model.as_deref().unwrap_or("-"),
            self.effort
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Habits {
    pub hand_back_turns: u64,
    pub max_turns: u64,
    pub builder_context: u64,
    pub parallel_cap: u64,
    pub read_lines: u64,
    pub cat_lines: u64,
    pub wait_secs: u64,
    pub bash_output_max_chars: u64,
    pub compact_window: BTreeMap<String, u64>,
}

pub const HABIT_KEYS: [&str; 8] = [
    "handBackTurns",
    "maxTurns",
    "builderContext",
    "parallelCap",
    "readLines",
    "catLines",
    "waitSecs",
    "bashOutputMaxChars",
];

impl Habits {
    pub fn from_value(v: &Value) -> Option<Habits> {
        let n = |k: &str| v.get(k).and_then(Value::as_u64);
        let compact = v
            .get("compactWindow")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_u64()?)))
                    .collect()
            })
            .unwrap_or_default();
        Some(Habits {
            hand_back_turns: n("handBackTurns")?,
            max_turns: n("maxTurns")?,
            builder_context: n("builderContext")?,
            parallel_cap: n("parallelCap")?,
            read_lines: n("readLines")?,
            cat_lines: n("catLines")?,
            wait_secs: n("waitSecs")?,
            bash_output_max_chars: n("bashOutputMaxChars")?,
            compact_window: compact,
        })
    }

    pub fn get(&self, key: &str) -> Option<u64> {
        Some(match key {
            "handBackTurns" => self.hand_back_turns,
            "maxTurns" => self.max_turns,
            "builderContext" => self.builder_context,
            "parallelCap" => self.parallel_cap,
            "readLines" => self.read_lines,
            "catLines" => self.cat_lines,
            "waitSecs" => self.wait_secs,
            "bashOutputMaxChars" => self.bash_output_max_chars,
            _ => return None,
        })
    }

    pub fn set(&mut self, key: &str, value: u64) -> bool {
        let slot = match key {
            "handBackTurns" => &mut self.hand_back_turns,
            "maxTurns" => &mut self.max_turns,
            "builderContext" => &mut self.builder_context,
            "parallelCap" => &mut self.parallel_cap,
            "readLines" => &mut self.read_lines,
            "catLines" => &mut self.cat_lines,
            "waitSecs" => &mut self.wait_secs,
            "bashOutputMaxChars" => &mut self.bash_output_max_chars,
            _ => return false,
        };
        *slot = value;
        true
    }
}

pub fn habit_range(key: &str) -> (u64, u64) {
    match key {
        "handBackTurns" => (10, 400),
        "maxTurns" => (20, 1000),
        "builderContext" => (20_000, 1_000_000),
        "parallelCap" => (1, 12),
        "readLines" => (20, 2000),
        "catLines" => (50, 5000),
        "waitSecs" => (30, 3600),
        "bashOutputMaxChars" => (2000, 100_000),
        _ => (0, 0),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReportLine {
    pub number: usize,
    pub text: String,
    pub fate: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub source: String,
    pub kept: Vec<String>,
    pub report: Vec<ReportLine>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stored {
    pub preset: Option<String>,
    pub picks: Option<String>,
    pub tools: Vec<Option<bool>>,
    pub app: Option<Value>,
}

pub fn parse_picks(text: &str) -> BTreeMap<(Role, Tool), Cell> {
    let mut out = BTreeMap::new();
    for part in text.split([';', '\n']) {
        let Some((left, right)) = part.split_once('=') else {
            continue;
        };
        let Some((role, tool)) = left.trim().split_once('.') else {
            continue;
        };
        let (Some(role), Some(tool)) = (Role::parse(role.trim()), parse_tool(tool.trim())) else {
            continue;
        };
        if let Some(cell) = Cell::parse(tool, right.trim()) {
            out.insert((role, tool), cell);
        }
    }
    out
}

pub fn picks_text(picks: &BTreeMap<(Role, Tool), Cell>) -> String {
    picks
        .iter()
        .map(|((role, tool), cell)| format!("{}.{}={cell}", role.name(), tool_key(*tool)))
        .collect::<Vec<_>>()
        .join("; ")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub preset: String,
    pub tools: BTreeMap<Tool, bool>,
    pub picks: BTreeMap<(Role, Tool), Cell>,
    pub habit_edits: BTreeMap<String, u64>,
    pub guards: BTreeMap<String, String>,
    pub imports: Vec<Imported>,
}

impl Default for Profile {
    fn default() -> Profile {
        Profile::preset(&super::presets::data().default)
    }
}

impl Profile {
    pub fn preset(name: &str) -> Profile {
        Profile {
            preset: name.to_string(),
            tools: Tool::ALL.iter().map(|t| (*t, true)).collect(),
            picks: BTreeMap::new(),
            habit_edits: BTreeMap::new(),
            guards: BTreeMap::new(),
            imports: Vec::new(),
        }
    }

    pub fn is_on(&self, tool: Tool) -> bool {
        self.tools.get(&tool).copied().unwrap_or(true)
    }

    pub fn is_custom(&self) -> bool {
        !self.picks.is_empty() || !self.habit_edits.is_empty()
    }

    pub fn load(settings_file: &Path) -> Profile {
        match crate::settings::Settings::open(settings_file) {
            Ok(store) => Profile::from_stored(&Stored {
                preset: store.get_str(KEY_PRESET),
                picks: store.get_str(KEY_PICKS),
                tools: KEY_TOOLS.iter().map(|k| store.get_bool(k)).collect(),
                app: store.raw(APP_KEY).cloned(),
            }),
            Err(_) => Profile::default(),
        }
    }

    pub fn from_stored(stored: &Stored) -> Profile {
        let data = super::presets::data();
        let mut p = Profile::default();
        if let Some(name) = stored
            .preset
            .as_deref()
            .filter(|n| data.presets.contains_key(*n))
        {
            p.preset = name.to_string();
        }
        for (tool, on) in Tool::ALL.iter().zip(&stored.tools) {
            if let Some(on) = on {
                p.tools.insert(*tool, *on);
            }
        }
        if let Some(text) = &stored.picks {
            p.picks = parse_picks(text);
        }
        if let Some(app) = stored.app.as_ref().and_then(Value::as_object) {
            if let Some(habits) = app.get("habits").and_then(Value::as_object) {
                for key in HABIT_KEYS {
                    if let Some(n) = habits.get(key).and_then(Value::as_u64) {
                        let (low, high) = habit_range(key);
                        p.habit_edits.insert(key.to_string(), n.clamp(low, high));
                    }
                }
            }
            if let Some(guards) = app.get("guards").and_then(Value::as_object) {
                for (k, v) in guards {
                    if let Some(level) = v.as_str().filter(|l| l.len() <= 16) {
                        p.guards.insert(k.clone(), level.to_string());
                    }
                }
            }
            p.imports = match app.get("imports").and_then(Value::as_array) {
                Some(list) => list.iter().filter_map(imported_of).collect(),
                None => app
                    .get("imported")
                    .and_then(imported_of)
                    .into_iter()
                    .collect(),
            };
        }
        p
    }

    pub fn to_stored(&self) -> Stored {
        let mut app = json!({
            "version": VERSION,
            "habits": self.habit_edits,
            "guards": self.guards,
        });
        if !self.imports.is_empty() {
            app["imports"] = Value::Array(
                self.imports
                    .iter()
                    .map(|i| {
                        json!({
                            "source": i.source,
                            "kept": i.kept,
                            "report": i.report.iter().map(|r| json!({"n": r.number, "text": r.text, "fate": r.fate})).collect::<Vec<_>>(),
                        })
                    })
                    .collect(),
            );
        }
        Stored {
            preset: Some(self.preset.clone()),
            picks: Some(picks_text(&self.picks)),
            tools: Tool::ALL.iter().map(|t| Some(self.is_on(*t))).collect(),
            app: Some(app),
        }
    }

    pub fn habits(&self) -> Habits {
        let data = super::presets::data();
        let base = data
            .presets
            .get(&self.preset)
            .or_else(|| data.presets.get(&data.default))
            .map(|p| p.habits.clone());
        let mut habits = base.unwrap_or_else(|| data.fallback_habits());
        for (k, v) in &self.habit_edits {
            habits.set(k, *v);
        }
        habits
    }

    pub fn cell(&self, role: Role, tool: Tool) -> Option<Cell> {
        if let Some(c) = self.picks.get(&(role, tool)) {
            return Some(c.clone());
        }
        let data = super::presets::data();
        let preset = data
            .presets
            .get(&self.preset)
            .or_else(|| data.presets.get(&data.default))?;
        preset.cells.get(&(role, tool)).cloned()
    }
}

fn imported_of(v: &Value) -> Option<Imported> {
    let source = v.get("source")?.as_str()?.to_string();
    let kept = v
        .get("kept")?
        .as_array()?
        .iter()
        .filter_map(|l| l.as_str().map(str::to_string))
        .collect();
    let report = v
        .get("report")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    Some(ReportLine {
                        number: r.get("n")?.as_u64()? as usize,
                        text: r.get("text")?.as_str()?.to_string(),
                        fate: r.get("fate")?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Imported {
        source,
        kept,
        report,
    })
}
