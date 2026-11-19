use notchling_engine::cost_profile::model::{CLAUDE_EFFORTS, CLAUDE_FAMILIES};
use serde_json::Value;

use super::profile::{Floor, Profile, TOOLS};

const LADDER: [&str; 6] = ["low", "medium", "high", "xhigh", "max", "ultra"];
pub const NO_MODEL: &str = "-";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelOption {
    pub id: String,
    pub efforts: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub tools: [Vec<ModelOption>; 3],
}

fn effort_rank(effort: &str) -> usize {
    LADDER.iter().position(|e| *e == effort).unwrap_or(0)
}

fn family_rank(model: &str) -> usize {
    CLAUDE_FAMILIES
        .iter()
        .position(|f| *f == model)
        .unwrap_or(0)
}

fn family_of(id: &str) -> Option<&'static str> {
    let family = id.strip_prefix("claude-")?.split('-').next()?;
    CLAUDE_FAMILIES.iter().copied().find(|f| *f == family)
}

fn sorted(mut efforts: Vec<String>) -> Vec<String> {
    efforts.sort_by_key(|e| effort_rank(e));
    efforts.dedup();
    efforts
}

fn listed(reply: Option<&Value>) -> Vec<(String, Vec<String>)> {
    reply
        .and_then(|r| r.get("models"))
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|m| {
                    let id = m.get("id")?.as_str()?.to_owned();
                    let efforts = m
                        .get("efforts")
                        .and_then(Value::as_array)
                        .map(|e| {
                            e.iter()
                                .filter_map(|x| x.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    Some((id, efforts))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn claude_options(reply: Option<&Value>) -> Vec<ModelOption> {
    let all: Vec<String> = CLAUDE_EFFORTS.iter().map(|e| (*e).to_owned()).collect();
    let models = listed(reply);
    let mut out = Vec::new();
    for family in CLAUDE_FAMILIES {
        let ours: Vec<&(String, Vec<String>)> = models
            .iter()
            .filter(|(id, _)| family_of(id) == Some(family))
            .collect();
        if ours.is_empty() && !models.is_empty() {
            continue;
        }
        let mut efforts: Vec<String> = ours
            .iter()
            .flat_map(|(_, e)| e.iter().cloned())
            .filter(|e| CLAUDE_EFFORTS.contains(&e.as_str()))
            .collect();
        if efforts.is_empty() {
            efforts = all.clone();
        }
        out.push(ModelOption {
            id: family.to_owned(),
            efforts: sorted(efforts),
        });
    }
    out
}

fn other_options(reply: Option<&Value>) -> Vec<ModelOption> {
    listed(reply)
        .into_iter()
        .map(|(id, efforts)| ModelOption {
            id,
            efforts: sorted(efforts),
        })
        .collect()
}

impl Options {
    pub fn from_replies(
        claude: Option<&Value>,
        codex: Option<&Value>,
        agy: Option<&Value>,
    ) -> Options {
        Options {
            tools: [
                claude_options(claude),
                other_options(codex),
                other_options(agy),
            ],
        }
    }

    pub fn known(profile: &Profile) -> Options {
        let mut options = Options {
            tools: [claude_options(None), Vec::new(), Vec::new()],
        };
        for row in &profile.rows {
            for (tool, cell) in row.cells.iter().enumerate().skip(1) {
                let Some(cell) = cell else { continue };
                let Some(model) = &cell.model else { continue };
                let list = &mut options.tools[tool];
                match list.iter_mut().find(|o| &o.id == model) {
                    Some(o) => {
                        if !o.efforts.contains(&cell.effort) {
                            o.efforts.push(cell.effort.clone());
                            o.efforts = sorted(std::mem::take(&mut o.efforts));
                        }
                    }
                    None => list.push(ModelOption {
                        id: model.clone(),
                        efforts: vec![cell.effort.clone()],
                    }),
                }
            }
        }
        options
    }

    pub fn or_known(mut self, profile: &Profile) -> Options {
        let known = Options::known(profile);
        for (tool, list) in self.tools.iter_mut().enumerate() {
            if list.is_empty() {
                *list = known.tools[tool].clone();
            }
        }
        self
    }

    pub fn models(&self, tool: usize, allow_none: bool) -> Vec<ModelOption> {
        let mut list = self.tools.get(tool).cloned().unwrap_or_default();
        if allow_none && tool > 0 {
            let mut efforts: Vec<String> = list.iter().flat_map(|o| o.efforts.clone()).collect();
            if efforts.is_empty() {
                efforts = LADDER[..4].iter().map(|e| (*e).to_owned()).collect();
            }
            list.insert(
                0,
                ModelOption {
                    id: NO_MODEL.to_owned(),
                    efforts: sorted(efforts),
                },
            );
        }
        list
    }
}

pub fn below_floor(tool: usize, floor: Option<&Floor>, model: &str, effort: &str) -> bool {
    let Some(floor) = floor.filter(|_| tool == 0) else {
        return false;
    };
    family_rank(model) < family_rank(&floor.model)
        || (model == floor.model && effort_rank(effort) < effort_rank(&floor.effort))
}

pub fn model_allowed(tool: usize, floor: Option<&Floor>, option: &ModelOption) -> bool {
    option
        .efforts
        .iter()
        .any(|e| !below_floor(tool, floor, &option.id, e))
}

pub fn effort_allowed(tool: usize, floor: Option<&Floor>, model: &str, effort: &str) -> bool {
    !below_floor(tool, floor, model, effort)
}

pub fn choose_model(
    tool: usize,
    floor: Option<&Floor>,
    option: &ModelOption,
    current_effort: &str,
) -> Option<String> {
    let fits = |e: &&String| !below_floor(tool, floor, &option.id, e);
    let want = effort_rank(current_effort);
    let effort = option
        .efforts
        .iter()
        .find(|e| e.as_str() == current_effort)
        .filter(fits)
        .or_else(|| {
            option
                .efforts
                .iter()
                .filter(fits)
                .find(|e| effort_rank(e) >= want)
        })
        .or_else(|| option.efforts.iter().rfind(fits))?;
    Some(format!("{} {}", option.id, effort))
}

pub fn pick_text(model: &str, effort: &str) -> String {
    [model, effort].join(" ")
}

pub fn tool_name(tool: usize) -> &'static str {
    TOOLS.get(tool).copied().unwrap_or_default()
}
