use notchling_locale as t;
use serde_json::Value;

use crate::acting::DETACHED_MESSAGE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillState {
    Installed,
    OutOfDate,
    NotInstalled,
    ToolMissing,
    LeftAlone,
}

impl SkillState {
    pub fn word(self) -> &'static str {
        match self {
            SkillState::Installed => t::settings::skill_installed(),
            SkillState::OutOfDate => t::settings::skill_out_of_date(),
            SkillState::NotInstalled => t::settings::skill_not_installed(),
            SkillState::ToolMissing => t::settings::skill_tool_missing(),
            SkillState::LeftAlone => t::settings::skill_left_alone(),
        }
    }

    pub fn of_plan(action: &str, tool_missing: bool) -> Option<SkillState> {
        match action {
            "unchanged" => Some(SkillState::Installed),
            "update" => Some(SkillState::OutOfDate),
            "write" => Some(SkillState::NotInstalled),
            "skip" if tool_missing => Some(SkillState::ToolMissing),
            "skip" => Some(SkillState::LeftAlone),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillTool {
    Claude,
    Codex,
    Antigravity,
}

impl SkillTool {
    pub const ALL: [SkillTool; 3] = [SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity];

    pub fn of(name: &str) -> Option<SkillTool> {
        SkillTool::ALL.into_iter().find(|t| t.id() == name)
    }

    pub fn id(self) -> &'static str {
        match self {
            SkillTool::Claude => "claude",
            SkillTool::Codex => "codex",
            SkillTool::Antigravity => "antigravity",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            SkillTool::Claude => t::settings::tool_claude_code(),
            SkillTool::Codex => t::settings::tool_codex(),
            SkillTool::Antigravity => t::settings::tool_antigravity(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillLine {
    pub name: String,
    pub state: SkillState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSkills {
    pub tool: SkillTool,
    pub lines: Vec<SkillLine>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkillsShown {
    Checking,
    Detached,
    Failed(String),
    Found {
        tools: Vec<ToolSkills>,
        install_on: bool,
    },
}

impl SkillsShown {
    pub fn from_reply(reply: Result<Value, String>) -> SkillsShown {
        let reply = match reply {
            Ok(reply) => reply,
            Err(e) if e == DETACHED_MESSAGE => return SkillsShown::Detached,
            Err(e) => return SkillsShown::Failed(e),
        };
        let Some(skills) = reply
            .get("plan")
            .and_then(|p| p.get("skills"))
            .and_then(Value::as_array)
        else {
            return SkillsShown::Failed(t::settings::reply_unreadable().into());
        };
        let mut tools: Vec<ToolSkills> = SkillTool::ALL
            .iter()
            .map(|&tool| ToolSkills {
                tool,
                lines: Vec::new(),
            })
            .collect();
        for item in skills {
            let text = |key: &str| item.get(key).and_then(Value::as_str);
            let (Some(name), Some(tool), Some(action)) =
                (text("name"), text("tool"), text("action"))
            else {
                continue;
            };
            let (Some(tool), Some(state)) = (
                SkillTool::of(tool),
                SkillState::of_plan(
                    action,
                    item.get("tool_missing").and_then(Value::as_bool) == Some(true),
                ),
            ) else {
                continue;
            };
            if let Some(group) = tools.iter_mut().find(|g| g.tool == tool) {
                group.lines.push(SkillLine {
                    name: name.to_owned(),
                    state,
                });
            }
        }
        tools.retain(|g| !g.lines.is_empty());
        SkillsShown::Found {
            tools,
            install_on: reply.get("on").and_then(Value::as_bool).unwrap_or(false),
        }
    }

    pub fn status(&self) -> Option<String> {
        match self {
            SkillsShown::Checking => Some(t::settings::skills_checking().into()),
            SkillsShown::Detached => Some(t::settings::skills_detached().into()),
            SkillsShown::Failed(e) => Some(e.clone()),
            SkillsShown::Found { .. } => None,
        }
    }

    pub fn names(&self) -> Vec<String> {
        let SkillsShown::Found { tools, .. } = self else {
            return Vec::new();
        };
        let mut names: Vec<String> = Vec::new();
        for line in tools.iter().flat_map(|t| &t.lines) {
            if !names.contains(&line.name) {
                names.push(line.name.clone());
            }
        }
        names
    }

    pub fn can_reinstall(&self) -> bool {
        matches!(
            self,
            SkillsShown::Found {
                install_on: true,
                ..
            }
        ) && !self.names().is_empty()
    }

    pub fn switch_note(&self) -> Option<&'static str> {
        match self {
            SkillsShown::Found {
                install_on: false, ..
            } => Some(t::settings::skills_switch_off()),
            _ => None,
        }
    }
}

pub fn beside_button(
    skills: &SkillsShown,
    result: Option<&(String, bool)>,
) -> Option<(String, bool)> {
    match result {
        Some((text, failed)) => Some((text.clone(), *failed)),
        None => skills.switch_note().map(|n| (n.to_owned(), false)),
    }
}

pub fn reinstall_result(reply: Result<Value, String>) -> Result<&'static str, String> {
    let reply = reply?;
    if reply.get("on").and_then(Value::as_bool) == Some(false) {
        return Err(t::settings::skills_switch_off().to_owned());
    }
    let failed = reply
        .get("plan")
        .and_then(|p| p.get("skills"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|s| {
            let why = s.get("error").and_then(Value::as_str)?;
            let name = s.get("name").and_then(Value::as_str).unwrap_or_default();
            Some(t::settings::skill_error(name, why))
        });
    if let Some(failed) = failed {
        return Err(failed);
    }
    if reply.get("applied").and_then(Value::as_bool) == Some(true) {
        Ok(t::settings::skills_reinstalled())
    } else {
        Err(t::settings::reply_unreadable().to_owned())
    }
}
