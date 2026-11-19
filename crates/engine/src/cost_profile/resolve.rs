use super::model::{Cell, Profile, Role, CLAUDE_EFFORTS, CLAUDE_FAMILIES};
use super::presets::data;
use crate::catalog::Tool;
use crate::catalog::{Catalog, LiveModel, Tier};
use crate::delegate::family::{self, Newest};
use crate::tools::codex::models::Model as CodexModel;
use std::collections::BTreeMap;

const EFFORT_LADDER: [&str; 6] = ["low", "medium", "high", "xhigh", "max", "ultra"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub id: String,
    pub efforts: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Available {
    pub claude: Option<BTreeMap<String, String>>,
    pub codex: Option<Vec<Offer>>,
    pub agy: Option<Vec<Offer>>,
}

pub fn claude_family(id: &str) -> Option<&str> {
    let rest = id.strip_prefix("claude-")?;
    let family = rest.split('-').next()?;
    CLAUDE_FAMILIES.contains(&family).then_some(family)
}

impl Available {
    pub fn from_catalog(catalog: &Catalog, live: &[LiveModel]) -> Available {
        let mut newest: BTreeMap<String, String> = BTreeMap::new();
        let mut older: BTreeMap<String, String> = BTreeMap::new();
        for info in catalog.models(Tool::Claude, live) {
            let Some(family) = claude_family(&info.id).map(str::to_string) else {
                continue;
            };
            let slot = if info.tier == Tier::Current {
                &mut newest
            } else {
                &mut older
            };
            slot.entry(family).or_insert(info.id);
        }
        for (family, id) in older {
            newest.entry(family).or_insert(id);
        }
        Available {
            claude: Some(newest),
            codex: None,
            agy: None,
        }
    }

    pub fn with_codex(mut self, models: &[CodexModel]) -> Available {
        if !models.is_empty() {
            self.codex = Some(
                models
                    .iter()
                    .map(|m| Offer {
                        id: m.slug.clone(),
                        efforts: m.efforts.clone(),
                    })
                    .collect(),
            );
        }
        self
    }

    pub fn with_agy(mut self, listed: &[(String, Vec<String>)]) -> Available {
        if !listed.is_empty() {
            self.agy = Some(
                listed
                    .iter()
                    .map(|(id, efforts)| Offer {
                        id: id.clone(),
                        efforts: efforts.clone(),
                    })
                    .collect(),
            );
        }
        self
    }

    pub fn claude_id(&self, family: &str) -> Option<&str> {
        self.claude.as_ref()?.get(family).map(String::as_str)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub code: &'static str,
    pub detail: String,
}

fn note(code: &'static str, detail: impl Into<String>) -> Note {
    Note {
        code,
        detail: detail.into(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub model: Option<String>,
    pub effort: String,
    pub notes: Vec<Note>,
    pub own: bool,
}

impl Resolved {
    pub fn text(&self) -> String {
        format!("{} {}", self.model.as_deref().unwrap_or("-"), self.effort)
    }
}

fn family_rank(family: &str) -> usize {
    CLAUDE_FAMILIES
        .iter()
        .position(|f| *f == family)
        .unwrap_or(0)
}

fn effort_rank(effort: &str) -> usize {
    CLAUDE_EFFORTS
        .iter()
        .position(|f| *f == effort)
        .unwrap_or(0)
}

fn claude_cell(role: Role, cell: &Cell, avail: &Available) -> Resolved {
    let mut model = cell.model.clone().unwrap_or_default();
    let mut effort = cell.effort.clone();
    let mut notes = Vec::new();
    if let Some(have) = &avail.claude {
        if !have.contains_key(&model) {
            let want = family_rank(&model);
            let pick = CLAUDE_FAMILIES
                .iter()
                .enumerate()
                .filter(|(_, f)| have.contains_key(**f))
                .min_by_key(|(i, _)| (*i < want, i.abs_diff(want)))
                .map(|(_, f)| f.to_string());
            if let Some(to) = pick {
                notes.push(note("model-missing", format!("{model} -> {to}")));
                model = to;
            }
        }
    }
    if let Some(floor) = data().floors.get(&role) {
        let below = family_rank(&model) < family_rank(&floor.model)
            || (model == floor.model && effort_rank(&effort) < effort_rank(&floor.effort));
        if below {
            notes.push(note(
                "floor",
                format!("{model} {effort} -> {} {}", floor.model, floor.effort),
            ));
            model = floor.model.clone();
            effort = floor.effort.clone();
        }
    }
    Resolved {
        model: Some(model),
        effort,
        notes,
        own: false,
    }
}

fn clamp_effort(effort: &str, offered: &[String]) -> Option<String> {
    if offered.is_empty() || offered.iter().any(|e| e == effort) {
        return None;
    }
    let at = |e: &str| EFFORT_LADDER.iter().position(|x| *x == e).unwrap_or(0);
    let want = at(effort);
    offered
        .iter()
        .filter(|e| at(e) <= want)
        .max_by_key(|e| at(e))
        .or_else(|| offered.iter().min_by_key(|e| at(e)))
        .cloned()
}

fn other_cell(cell: &Cell, offers: &Option<Vec<Offer>>) -> Resolved {
    let mut notes = Vec::new();
    let mut model = cell.model.clone();
    let mut effort = cell.effort.clone();
    let Some(offers) = offers else {
        return Resolved {
            model,
            effort,
            notes: vec![note("unchecked", "")],
            own: false,
        };
    };
    if let Some(want) = model.clone() {
        if !offers.iter().any(|o| o.id == want) {
            let ids: Vec<String> = offers.iter().map(|o| o.id.clone()).collect();
            let to = match family::newest(&want, &ids, |_| false) {
                Newest::One(id, _) => Some(id.to_string()),
                Newest::Nothing => ids.first().cloned(),
            };
            if let Some(to) = to {
                notes.push(note("model-missing", format!("{want} -> {to}")));
                model = Some(to);
            }
        }
        if let Some(offer) = offers.iter().find(|o| Some(&o.id) == model.as_ref()) {
            if let Some(to) = clamp_effort(&effort, &offer.efforts) {
                notes.push(note("effort-clamped", format!("{effort} -> {to}")));
                effort = to;
            }
        }
    }
    Resolved {
        model,
        effort,
        notes,
        own: false,
    }
}

pub fn resolve(profile: &Profile, role: Role, tool: Tool, avail: &Available) -> Option<Resolved> {
    if !profile.is_on(tool) {
        return None;
    }
    let own = profile.picks.contains_key(&(role, tool));
    let cell = profile.cell(role, tool)?;
    let mut resolved = match tool {
        Tool::Claude => claude_cell(role, &cell, avail),
        Tool::Codex => other_cell(&cell, &avail.codex),
        Tool::Antigravity => other_cell(&cell, &avail.agy),
    };
    resolved.own = own;
    Some(resolved)
}

pub fn claude_ids(avail: &Available) -> BTreeMap<String, String> {
    avail.claude.clone().unwrap_or_default()
}
