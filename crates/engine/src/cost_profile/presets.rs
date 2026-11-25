use super::model::{parse_tool, Cell, Habits, Role};
use crate::catalog::Tool;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const BUILT_IN: &str = include_str!("../../data/profiles.json");

#[derive(Clone, Debug)]
pub struct Preset {
    pub cells: BTreeMap<(Role, Tool), Cell>,
    pub habits: Habits,
}

#[derive(Clone, Debug)]
pub struct Floor {
    pub model: String,
    pub effort: String,
}

#[derive(Clone, Debug)]
pub struct PstackRole {
    pub role: String,
    pub uses: Vec<Role>,
}

#[derive(Clone, Debug)]
pub struct Data {
    pub default: String,
    pub floors: BTreeMap<Role, Floor>,
    pub presets: BTreeMap<String, Preset>,
    pub pstack: Vec<PstackRole>,
    pub import_words: Vec<(Role, Vec<String>)>,
}

impl Data {
    pub fn preset_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.presets.keys().map(String::as_str).collect();
        names.sort_by_key(|n| (*n != "saver", *n != self.default, n.to_string()));
        names
    }

    pub fn fallback_habits(&self) -> Habits {
        self.presets
            .values()
            .next()
            .map(|p| p.habits.clone())
            .unwrap_or(Habits {
                hand_back_turns: 80,
                max_turns: 140,
                builder_context: 150_000,
                parallel_cap: 5,
                read_lines: 150,
                cat_lines: 300,
                wait_secs: 270,
                bash_output_max_chars: 12_000,
                compact_window: BTreeMap::new(),
            })
    }
}

fn floor_of(text: &str) -> Option<Floor> {
    let (model, effort) = text.split_once(' ')?;
    Some(Floor {
        model: model.to_string(),
        effort: effort.to_string(),
    })
}

pub fn parse(text: &str) -> Option<Data> {
    let v: Value = serde_json::from_str(text).ok()?;
    let default = v.get("default")?.as_str()?.to_string();
    let floors = v
        .get("floors")?
        .as_object()?
        .iter()
        .filter_map(|(k, f)| Some((Role::parse(k)?, floor_of(f.as_str()?)?)))
        .collect();
    let mut presets = BTreeMap::new();
    for (name, p) in v.get("presets")?.as_object()? {
        let mut cells = BTreeMap::new();
        for (role, tools) in p.get("cells")?.as_object()? {
            let role = Role::parse(role)?;
            for (tool, text) in tools.as_object()? {
                let tool = parse_tool(tool)?;
                cells.insert((role, tool), Cell::parse(tool, text.as_str()?)?);
            }
        }
        presets.insert(
            name.clone(),
            Preset {
                cells,
                habits: Habits::from_value(p.get("habits")?)?,
            },
        );
    }
    let pstack = v
        .get("pstack")?
        .as_array()?
        .iter()
        .filter_map(|r| {
            Some(PstackRole {
                role: r.get("role")?.as_str()?.to_string(),
                uses: r
                    .get("use")?
                    .as_array()?
                    .iter()
                    .filter_map(|u| Role::parse(u.as_str()?))
                    .collect(),
            })
        })
        .collect();
    let import_words = v
        .get("importWords")?
        .as_array()?
        .iter()
        .filter_map(|r| {
            Some((
                Role::parse(r.get("role")?.as_str()?)?,
                r.get("words")?
                    .as_array()?
                    .iter()
                    .filter_map(|w| w.as_str().map(str::to_string))
                    .collect(),
            ))
        })
        .collect();
    presets.contains_key(&default).then_some(Data {
        default,
        floors,
        presets,
        pstack,
        import_words,
    })
}

pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| parse(BUILT_IN).expect("data/profiles.json is built in and valid"))
}
