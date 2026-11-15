//! What each delegate model is for, from the catalog's `picks` and `not_for` lists.
//! Additive: readers that only know `prices` skip both keys.

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub model: String,
    pub effort: String,
    pub roles: Vec<String>,
    pub note: String,
}

/// A model name fragment, an effort, or both that a role should not use.
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
            Some(Pick {
                model: text(p, "model")?,
                effort: text(p, "effort")?,
                roles: strings(p, "roles"),
                note: text(p, "use").unwrap_or_default(),
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
        .collect();
    ToolRoles { picks, avoids }
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

    /// The data file with the user's `catalog.json` laid over it: a pick replaces the
    /// compiled-in ones for the same model (an empty `roles` removes them), a `not_for`
    /// entry replaces the one with the same `match` and `effort`. Bad entries are skipped.
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

    /// `tool` is the catalog's name: `codex` or `antigravity`.
    pub fn tool(&self, tool: &str) -> &ToolRoles {
        match tool {
            "codex" => &self.codex,
            _ => &self.antigravity,
        }
    }

    /// Every role any model is marked for, in first-seen order.
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
        }
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

    pub fn picks_of(&self, model: &str) -> Vec<&Pick> {
        self.picks.iter().filter(|p| p.model == model).collect()
    }

    /// Roles a model name is marked not for.
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

    /// Efforts no model should use for a role, as `(effort, roles)`.
    pub fn not_for_efforts(&self) -> Vec<(String, Vec<String>)> {
        self.avoids
            .iter()
            .filter(|a| a.matching.is_none())
            .filter_map(|a| Some((a.effort.clone()?, a.roles.clone())))
            .collect()
    }
}

#[cfg(test)]
#[path = "roles_tests.rs"]
mod tests;
