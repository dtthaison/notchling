use super::keys::{KeySpec, Kind};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Unset,
    Free,
    Ask,
    Never,
}

impl Access {
    pub fn as_str(self) -> &'static str {
        match self {
            Access::Free => "free",
            Access::Ask => "ask",
            Access::Unset | Access::Never => "never",
        }
    }

    pub fn shown(self) -> bool {
        matches!(self, Access::Free | Access::Ask)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchSpec {
    pub name: &'static str,
    pub key: &'static str,
    pub title: &'static str,
    pub about: &'static str,
}

pub const SWITCHES: [SwitchSpec; 5] = [
    SwitchSpec {
        name: "acting",
        key: "switch.acting",
        title: "Act on Sessions",
        about: "Let Notchling reply to, answer and control sessions, and run the hook",
    },
    SwitchSpec {
        name: "bridge",
        key: "switch.bridge",
        title: "Phone Bridge",
        about: "Let paired phones follow and answer sessions",
    },
    SwitchSpec {
        name: "accounts",
        key: "switch.accounts",
        title: "Logins and Lanes",
        about: "Let Notchling switch accounts, lanes and providers",
    },
    SwitchSpec {
        name: "vault",
        key: "switch.vault",
        title: "Secret Vault",
        about: "Let agents use saved secrets, with the user's approval where asked",
    },
    SwitchSpec {
        name: "install",
        key: "switch.install",
        title: "Install Hooks and Skills",
        about: "Let Notchling write its hook and skills into the agents' folders",
    },
];

pub const SWITCH_WHY: &str = "a switch: only the user turns it on or off, in Notchling's Settings";

pub fn switch(key: &str) -> Option<&'static SwitchSpec> {
    SWITCHES
        .iter()
        .find(|s| s.key == key || (s.name == key && super::keys::spec(key).is_none()))
}

impl KeySpec {
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            Kind::Bool => "bool",
            Kind::Int => "int",
            Kind::Double => "number",
            Kind::String => "string",
            Kind::StringList => "list",
            Kind::Dict => "object",
            Kind::Json | Kind::Any => "json",
        }
    }

    pub fn shown_title(&self) -> &'static str {
        if self.title.is_empty() {
            self.name
        } else {
            self.title
        }
    }

    pub fn from_text(&self, text: &str) -> Value {
        let t = text.trim();
        let lower = t.to_ascii_lowercase();
        match self.kind {
            Kind::Bool => match lower.as_str() {
                "true" | "on" | "yes" => json!(true),
                "false" | "off" | "no" => json!(false),
                _ => json!(text),
            },
            Kind::Int => t.parse::<i64>().map(|n| json!(n)).unwrap_or(json!(text)),
            Kind::Double => t
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .map(|n| json!(n))
                .unwrap_or(json!(text)),
            Kind::String => match self.allowed.iter().find(|a| a.eq_ignore_ascii_case(t)) {
                Some(allowed) => json!(allowed),
                None => json!(text),
            },
            Kind::StringList | Kind::Dict | Kind::Json | Kind::Any => {
                serde_json::from_str(t).unwrap_or(json!(text))
            }
        }
    }

    pub fn read_value(&self, value: Value) -> Value {
        match value {
            Value::String(text) => self.from_text(&text),
            other => other,
        }
    }

    pub fn expects(&self) -> String {
        match (self.kind, self.range) {
            (Kind::Bool, _) => "true or false".into(),
            (Kind::Int, Some((low, high))) => format!("a whole number from {low} to {high}"),
            (Kind::Int, None) => "a whole number".into(),
            (Kind::Double, _) => "a number".into(),
            (Kind::String, _) if !self.allowed.is_empty() => {
                format!("one of {}", self.allowed.join(", "))
            }
            (Kind::String, _) => "text".into(),
            (Kind::StringList, _) => "a list of texts".into(),
            (Kind::Dict, _) => "an object".into(),
            (Kind::Json | Kind::Any, _) => "a JSON value".into(),
        }
    }

    pub fn check(&self, value: &Value) -> Result<(), String> {
        let in_range = match (self.kind, self.range) {
            (Kind::Int, Some((low, high))) => {
                value.as_i64().is_some_and(|n| (low..=high).contains(&n))
            }
            (Kind::Double, _) => value.as_f64().is_some_and(f64::is_finite),
            _ => true,
        };
        if self.accepts(value) && in_range {
            Ok(())
        } else {
            Err(format!("{} takes {}", self.name, self.expects()))
        }
    }
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
