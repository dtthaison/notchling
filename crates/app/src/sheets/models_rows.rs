//! Settings > Skills & Models, Models by Job and Override: what the daemon's `catalog.roles`
//! says each job uses, where that choice came from, and the user's override file.

use notchling_locale as t;
use serde_json::Value;

use super::client_model::shown_path;
use super::skills_rows::SkillTool;
use crate::acting::DETACHED_MESSAGE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    BuiltIn,
    Override,
    Live,
}

impl Source {
    pub fn word(self) -> &'static str {
        match self {
            Source::BuiltIn => t::settings::model_source_built_in(),
            Source::Override => t::settings::model_source_override(),
            Source::Live => t::settings::model_source_live(),
        }
    }

    fn of(name: &str) -> Source {
        match name {
            "override" => Source::Override,
            "live" => Source::Live,
            _ => Source::BuiltIn,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceLine {
    /// The tool and the effort, as one line.
    pub tool_effort: String,
    /// The model to use, or the line that says none is available.
    pub model: String,
    pub has_model: bool,
    pub source: Source,
    /// The engine's note that the marked model was replaced or has a newer sibling.
    pub note: Option<String>,
    /// The catalog's advice for this pick.
    pub advice: Option<String>,
    pub unchecked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleLine {
    pub title: String,
    pub choices: Vec<ChoiceLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverrideState {
    Missing,
    Found,
    Unreadable,
}

impl OverrideState {
    pub fn line(self) -> &'static str {
        match self {
            OverrideState::Missing => t::settings::override_missing(),
            OverrideState::Found => t::settings::override_found(),
            OverrideState::Unreadable => t::settings::override_unreadable(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverrideInfo {
    /// The file's path as the daemon says it, for opening.
    pub path: Option<String>,
    /// The same with the home folder as `~`, for showing.
    pub shown: String,
    pub state: OverrideState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelsShown {
    Checking,
    Detached,
    Failed(String),
    Found {
        roles: Vec<RoleLine>,
        file: OverrideInfo,
    },
}

fn role_title(role: &str) -> String {
    match role {
        "review" => t::settings::role_review().to_owned(),
        "review-final" => t::settings::role_review_final().to_owned(),
        "review-secrets" => t::settings::role_review_secrets().to_owned(),
        "spec-check" => t::settings::role_spec_check().to_owned(),
        other => other.to_owned(),
    }
}

fn tool_title(tool: &str) -> String {
    SkillTool::of(tool).map_or_else(|| tool.to_owned(), |t| t.title().to_owned())
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
}

fn choice(c: &Value) -> ChoiceLine {
    let model = text(c, "model");
    ChoiceLine {
        tool_effort: t::settings::model_effort(
            &tool_title(&text(c, "tool").unwrap_or_default()),
            &text(c, "effort").unwrap_or_default(),
        ),
        has_model: model.is_some(),
        model: model.unwrap_or_else(|| t::settings::model_none().to_owned()),
        source: Source::of(&text(c, "source").unwrap_or_default()),
        note: text(c, "note"),
        advice: text(c, "advice"),
        unchecked: c.get("checked").and_then(Value::as_bool) == Some(false),
    }
}

impl ModelsShown {
    pub fn from_reply(reply: Result<Value, String>, home: &str) -> ModelsShown {
        let reply = match reply {
            Ok(reply) => reply,
            Err(e) if e == DETACHED_MESSAGE => return ModelsShown::Detached,
            Err(e) => return ModelsShown::Failed(e),
        };
        let Some(roles) = reply.get("roles").and_then(Value::as_array) else {
            return ModelsShown::Failed(t::settings::reply_unreadable().into());
        };
        let roles = roles
            .iter()
            .filter_map(|r| {
                Some(RoleLine {
                    title: role_title(r.get("role")?.as_str()?),
                    choices: r.get("choices")?.as_array()?.iter().map(choice).collect(),
                })
            })
            .collect();
        let file = reply.get("override");
        let path = file.and_then(|f| text(f, "path"));
        let state = match file.and_then(|f| text(f, "state")).as_deref() {
            Some("found") => OverrideState::Found,
            Some("unreadable") => OverrideState::Unreadable,
            _ => OverrideState::Missing,
        };
        ModelsShown::Found {
            roles,
            file: OverrideInfo {
                shown: path
                    .as_deref()
                    .map(|p| shown_path(p, home))
                    .unwrap_or_default(),
                path,
                state,
            },
        }
    }

    pub fn status(&self) -> Option<String> {
        match self {
            ModelsShown::Checking => Some(t::settings::models_checking().into()),
            ModelsShown::Detached => Some(t::settings::models_detached().into()),
            ModelsShown::Failed(e) => Some(e.clone()),
            ModelsShown::Found { .. } => None,
        }
    }

    pub fn file(&self) -> Option<&OverrideInfo> {
        match self {
            ModelsShown::Found { file, .. } => Some(file),
            _ => None,
        }
    }
}
