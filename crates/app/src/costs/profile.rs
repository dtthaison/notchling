use notchling_locale as t;
use serde_json::Value;

pub const TOOLS: [&str; 3] = ["claude", "codex", "agy"];
pub const ROLES: [&str; 7] = ["read", "code", "fix", "judge", "hardest", "review", "image"];
pub const PRESETS: [&str; 3] = ["saver", "balanced", "quality"];
pub const CUSTOM: &str = "custom";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub code: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub model: Option<String>,
    pub effort: String,
    pub own: bool,
    pub notes: Vec<Note>,
}

impl Cell {
    pub fn text(&self) -> String {
        format!("{} {}", self.model.as_deref().unwrap_or("-"), self.effort)
    }

    pub fn has_note(&self, code: &str) -> bool {
        self.notes.iter().any(|n| n.code == code)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Floor {
    pub model: String,
    pub effort: String,
}

impl Floor {
    pub fn parse(text: &str) -> Option<Floor> {
        let (model, effort) = text.split_once(' ')?;
        Some(Floor {
            model: model.to_owned(),
            effort: effort.to_owned(),
        })
    }

    pub fn text(&self) -> String {
        format!("{} {}", self.model, self.effort)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub role: String,
    pub cells: [Option<Cell>; 3],
    pub floor: Option<Floor>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedLine {
    pub number: u64,
    pub text: String,
    pub fate: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    pub source: String,
    pub lines: Vec<ImportedLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub key: &'static str,
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub preset: String,
    pub custom: bool,
    pub tools: [bool; 3],
    pub rows: Vec<Row>,
    pub habits: Vec<(String, u64)>,
    pub imports: Vec<Imported>,
}

pub fn role_title(role: &str) -> String {
    match role {
        "read" => t::costs::role_read(),
        "code" => t::costs::role_code(),
        "fix" => t::costs::role_fix(),
        "judge" => t::costs::role_judge(),
        "hardest" => t::costs::role_hardest(),
        "review" => t::costs::role_review(),
        "image" => t::costs::role_image(),
        _ => role,
    }
    .to_owned()
}

pub fn tool_title(tool: usize) -> &'static str {
    match tool {
        0 => t::settings::tool_claude_code(),
        1 => t::settings::tool_codex(),
        _ => t::settings::tool_antigravity(),
    }
}

pub fn note_text(note: &Note) -> Option<String> {
    Some(match note.code.as_str() {
        "floor" => t::costs::note_floor(&note.detail),
        "model-missing" => t::costs::note_model_missing(&note.detail),
        "effort-clamped" => t::costs::note_effort_clamped(&note.detail),
        "unchecked" => t::costs::note_unchecked().to_owned(),
        _ => return None,
    })
}

fn cell_of(v: &Value) -> Option<Cell> {
    if v.is_null() {
        return None;
    }
    let notes = v
        .get("notes")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|n| Note {
                    code: n["code"].as_str().unwrap_or_default().to_owned(),
                    detail: n["detail"].as_str().unwrap_or_default().to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Cell {
        model: v.get("model").and_then(Value::as_str).map(str::to_owned),
        effort: v.get("effort")?.as_str()?.to_owned(),
        own: v.get("own").and_then(Value::as_bool).unwrap_or(false),
        notes,
    })
}

pub fn imported_of(v: &Value) -> Option<Imported> {
    let lines = v
        .get("report")?
        .as_array()?
        .iter()
        .filter_map(|r| {
            Some(ImportedLine {
                number: r.get("line")?.as_u64()?,
                text: r.get("text")?.as_str()?.to_owned(),
                fate: r.get("fate")?.as_str()?.to_owned(),
            })
        })
        .collect();
    Some(Imported {
        source: v.get("source")?.as_str()?.to_owned(),
        lines,
    })
}

pub fn imports_of(v: &Value) -> Vec<Imported> {
    match v.get("imports").and_then(Value::as_array) {
        Some(list) => list.iter().filter_map(imported_of).collect(),
        None => v
            .get("imported")
            .and_then(imported_of)
            .into_iter()
            .collect(),
    }
}

const HABITS: [&str; 5] = [
    "handBackTurns",
    "parallelCap",
    "readLines",
    "waitSecs",
    "bashOutputMaxChars",
];

impl Profile {
    pub fn from_reply(reply: &Value) -> Option<Profile> {
        let p = reply.get("profile").unwrap_or(reply);
        let table = p.get("table")?.as_object()?;
        let floors = p.get("floors").and_then(Value::as_object);
        let rows = ROLES
            .iter()
            .filter_map(|role| {
                let by_tool = table.get(*role)?;
                let mut cells: [Option<Cell>; 3] = [None, None, None];
                for (i, tool) in TOOLS.iter().enumerate() {
                    cells[i] = by_tool.get(*tool).and_then(cell_of);
                }
                let floor = floors
                    .and_then(|f| f.get(*role))
                    .and_then(Value::as_str)
                    .and_then(Floor::parse);
                Some(Row {
                    role: (*role).to_owned(),
                    cells,
                    floor,
                })
            })
            .collect();
        let tools_on = p.get("tools");
        let mut tools = [true; 3];
        for (i, tool) in TOOLS.iter().enumerate() {
            if let Some(on) = tools_on.and_then(|t| t.get(*tool)).and_then(Value::as_bool) {
                tools[i] = on;
            }
        }
        let numbers = p.get("habits");
        let habits = HABITS
            .iter()
            .filter_map(|k| Some(((*k).to_owned(), numbers?.get(*k)?.as_u64()?)))
            .collect();
        Some(Profile {
            preset: p.get("preset")?.as_str()?.to_owned(),
            custom: p.get("custom").and_then(Value::as_bool).unwrap_or(false),
            tools,
            rows,
            habits,
            imports: imports_of(p),
        })
    }

    pub fn segments(&self) -> Vec<Segment> {
        let mut out: Vec<Segment> = PRESETS
            .iter()
            .map(|key| Segment {
                key,
                selected: !self.custom && self.preset == *key,
            })
            .collect();
        if self.custom {
            out.push(Segment {
                key: CUSTOM,
                selected: true,
            });
        }
        out
    }

    pub fn row(&self, role: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.role == role)
    }

    pub fn cell(&self, role: &str, tool: usize) -> Option<&Cell> {
        self.row(role)?.cells.get(tool)?.as_ref()
    }

    pub fn habit_lines(&self) -> Vec<String> {
        self.habits
            .iter()
            .filter_map(|(key, n)| {
                let n = *n;
                Some(match key.as_str() {
                    "handBackTurns" => t::costs::habit_hand_back(n),
                    "parallelCap" => t::costs::habit_parallel(n),
                    "readLines" => t::costs::habit_read(n),
                    "waitSecs" => t::costs::habit_wait(n),
                    "bashOutputMaxChars" => t::costs::habit_output(n),
                    _ => return None,
                })
            })
            .collect()
    }

    pub fn edited_cells(&self) -> usize {
        self.rows
            .iter()
            .flat_map(|r| r.cells.iter().flatten())
            .filter(|c| c.own)
            .count()
    }
}
