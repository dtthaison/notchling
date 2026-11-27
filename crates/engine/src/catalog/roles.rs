use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub tool: String,
    pub model: String,
    pub effort: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ladder {
    pub role: String,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub model: String,
    pub effort: String,
    pub roles: Vec<String>,
    pub note: String,
    pub then: Vec<Ladder>,
}

impl Pick {
    pub fn ladder(&self, role: &str) -> &[Step] {
        self.then
            .iter()
            .find(|l| l.role == role)
            .map(|l| l.steps.as_slice())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Avoid {
    pub matching: Option<String>,
    pub effort: Option<String>,
    pub roles: Vec<String>,
    pub note: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolRoles {
    pub picks: Vec<Pick>,
    pub avoids: Vec<Avoid>,
    users: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roles {
    codex: ToolRoles,
    antigravity: ToolRoles,
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_lowercase)
        .collect()
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn steps_of(v: &Value) -> Vec<Ladder> {
    let Some(map) = v.get("then").and_then(Value::as_object) else {
        return Vec::new();
    };
    map.iter()
        .map(|(role, list)| Ladder {
            role: role.to_lowercase(),
            steps: list
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    let tool = match text(s, "tool")?.to_lowercase().as_str() {
                        "codex" => "codex",
                        "agy" | "antigravity" => "agy",
                        _ => return None,
                    };
                    Some(Step {
                        tool: tool.to_string(),
                        model: text(s, "model")?,
                        effort: text(s, "effort").unwrap_or_default(),
                    })
                })
                .collect(),
        })
        .collect()
}

fn parse_tool(section: &Value, keep_empty: bool) -> ToolRoles {
    let list = |key: &str| {
        section
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let picks = list("picks")
        .iter()
        .filter_map(|p| {
            let roles = strings(p, "roles");
            let effort = match text(p, "effort") {
                Some(effort) => effort,
                None if keep_empty && roles.is_empty() => String::new(),
                None => return None,
            };
            Some(Pick {
                model: text(p, "model")?,
                effort,
                roles,
                note: text(p, "use").unwrap_or_default(),
                then: steps_of(p),
            })
        })
        .filter(|p| keep_empty || !p.roles.is_empty())
        .collect();
    let avoids = list("not_for")
        .iter()
        .filter_map(|a| {
            let matching = text(a, "match").map(|m| m.to_lowercase());
            let effort = text(a, "effort").map(|e| e.to_lowercase());
            if matching.is_none() && effort.is_none() {
                return None;
            }
            Some(Avoid {
                matching,
                effort,
                roles: strings(a, "roles"),
                note: text(a, "use").unwrap_or_default(),
            })
        })
        .filter(|a| keep_empty || !a.roles.is_empty())
        .collect();
    ToolRoles {
        picks,
        avoids,
        users: Vec::new(),
    }
}

pub fn covers(marked: &str, role: &str) -> bool {
    role == marked
        || role
            .strip_prefix(marked)
            .is_some_and(|rest| rest.starts_with('-'))
}

impl Roles {
    pub fn parse(root: &Value) -> Roles {
        let section = |name: &str| {
            root.get(name)
                .map(|s| parse_tool(s, false))
                .unwrap_or_default()
        };
        Roles {
            codex: section("codex"),
            antigravity: section("antigravity"),
        }
    }

    pub fn built_in() -> Roles {
        serde_json::from_str::<Value>(super::BUILT_IN)
            .map(|root| Roles::parse(&root))
            .unwrap_or_default()
    }

    pub fn load(override_file: Option<&std::path::Path>) -> Roles {
        let mut roles = Roles::built_in();
        let root = override_file
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        if let Some(root) = root {
            for (name, tool) in [
                ("codex", &mut roles.codex),
                ("antigravity", &mut roles.antigravity),
            ] {
                if let Some(section) = root.get(name) {
                    tool.overlay(parse_tool(section, true));
                }
            }
        }
        roles
    }

    pub fn tool(&self, tool: &str) -> &ToolRoles {
        match tool {
            "codex" => &self.codex,
            _ => &self.antigravity,
        }
    }

    pub fn all_roles(&self) -> Vec<String> {
        let mut roles: Vec<String> = Vec::new();
        for tool in [&self.codex, &self.antigravity] {
            for pick in &tool.picks {
                for role in &pick.roles {
                    if !roles.contains(role) {
                        roles.push(role.clone());
                    }
                }
            }
        }
        roles
    }
}

impl ToolRoles {
    fn overlay(&mut self, over: ToolRoles) {
        for model in over.picks.iter().map(|p| p.model.clone()) {
            self.picks.retain(|p| p.model != model);
            self.users.retain(|u| *u != model);
        }
        self.users.extend(
            over.picks
                .iter()
                .filter(|p| !p.roles.is_empty())
                .map(|p| p.model.clone()),
        );
        self.picks
            .extend(over.picks.into_iter().filter(|p| !p.roles.is_empty()));
        for a in over.avoids {
            self.avoids
                .retain(|b| (&b.matching, &b.effort) != (&a.matching, &a.effort));
            if !a.roles.is_empty() {
                self.avoids.push(a);
            }
        }
    }

    pub fn picks_for(&self, role: &str) -> Vec<&Pick> {
        self.picks
            .iter()
            .filter(|p| p.roles.iter().any(|r| r == role))
            .collect()
    }

    pub fn users_picks_for(&self, role: &str) -> Vec<&Pick> {
        self.picks_for(role)
            .into_iter()
            .filter(|p| self.users.contains(&p.model))
            .collect()
    }

    pub fn picks_of(&self, model: &str) -> Vec<&Pick> {
        self.picks.iter().filter(|p| p.model == model).collect()
    }

    pub fn not_for(&self, model: &str) -> Vec<String> {
        let model = model.to_lowercase();
        let mut roles: Vec<String> = Vec::new();
        for a in &self.avoids {
            let hit = a.effort.is_none() && a.matching.as_ref().is_some_and(|m| model.contains(m));
            if hit {
                for r in &a.roles {
                    if !roles.contains(r) {
                        roles.push(r.clone());
                    }
                }
            }
        }
        roles
    }

    pub fn excluded(&self, model: &str, effort: &str, role: &str) -> bool {
        let model = model.to_lowercase();
        let effort = effort.to_lowercase();
        self.avoids.iter().any(|a| {
            a.roles.iter().any(|r| covers(r, role))
                && a.matching.as_ref().is_none_or(|m| model.contains(m))
                && a.effort.as_ref().is_none_or(|e| *e == effort)
        })
    }

    pub fn not_for_efforts(&self) -> Vec<(Option<String>, String, Vec<String>)> {
        self.avoids
            .iter()
            .filter_map(|a| Some((a.matching.clone(), a.effort.clone()?, a.roles.clone())))
            .collect()
    }
}

#[cfg(test)]
#[path = "roles_tests.rs"]
mod tests;
