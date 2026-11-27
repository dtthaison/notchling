use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SqlArg {
    Text(String),
    Real(f64),
}

pub const NAMES: [&str; 7] = [
    "tool", "project", "account", "section", "owner", "kind", "state",
];

impl Filter {
    pub fn from_value(value: &Value) -> Result<Filter, String> {
        let map = match value {
            Value::Null => return Ok(Filter::default()),
            Value::Object(map) => map,
            _ => return Err("filter takes an object".into()),
        };
        let mut out = Filter::default();
        for name in NAMES {
            let list = match map.get(name) {
                None | Some(Value::Null) => continue,
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|v| v.as_str().map(str::to_owned))
                    .collect::<Option<Vec<String>>>()
                    .ok_or_else(|| format!("filter.{name} takes [string]"))?,
                Some(_) => return Err(format!("filter.{name} takes [string]")),
            };
            if !list.is_empty() {
                *out.slot(name) = Some(list);
            }
        }
        for (name, slot) in [("after", &mut out.after), ("before", &mut out.before)] {
            *slot = match map.get(name) {
                None | Some(Value::Null) => None,
                Some(v) => Some(
                    v.as_f64()
                        .filter(|n| n.is_finite())
                        .ok_or_else(|| format!("filter.{name} takes seconds"))?,
                ),
            };
        }
        Ok(out)
    }

    fn slot(&mut self, name: &str) -> &mut Option<Vec<String>> {
        match name {
            "tool" => &mut self.tool,
            "project" => &mut self.project,
            "account" => &mut self.account,
            "section" => &mut self.section,
            "owner" => &mut self.owner,
            "kind" => &mut self.kind,
            _ => &mut self.state,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.names().is_empty() && self.after.is_none() && self.before.is_none()
    }

    pub fn names(&self) -> Vec<(&'static str, &[String])> {
        [
            ("tool", &self.tool),
            ("project", &self.project),
            ("account", &self.account),
            ("section", &self.section),
            ("owner", &self.owner),
            ("kind", &self.kind),
            ("state", &self.state),
        ]
        .into_iter()
        .filter_map(|(name, list)| list.as_deref().map(|l| (name, l)))
        .collect()
    }

    pub fn matches(&self, facets: &[(&str, &str)], at: Option<f64>) -> bool {
        for (name, wanted) in self.names() {
            let hit = facets
                .iter()
                .any(|(n, v)| *n == name && wanted.iter().any(|w| w == v));
            if !hit {
                return false;
            }
        }
        if self.after.is_some() || self.before.is_some() {
            let Some(at) = at else {
                return false;
            };
            if self.after.is_some_and(|a| at < a) || self.before.is_some_and(|b| at >= b) {
                return false;
            }
        }
        true
    }

    pub fn sql(&self, doc: &str, at: &str) -> (String, Vec<SqlArg>) {
        let mut sql = String::new();
        let mut args = Vec::new();
        for (name, values) in self.names() {
            let marks = vec!["?"; values.len()].join(", ");
            sql.push_str(&format!(
                " AND {doc} IN (SELECT doc FROM facets WHERE name = ? AND value IN ({marks}))"
            ));
            args.push(SqlArg::Text(name.to_owned()));
            args.extend(values.iter().cloned().map(SqlArg::Text));
        }
        if let Some(after) = self.after {
            sql.push_str(&format!(" AND {at} >= ?"));
            args.push(SqlArg::Real(after));
        }
        if let Some(before) = self.before {
            sql.push_str(&format!(" AND {at} < ?"));
            args.push(SqlArg::Real(before));
        }
        (sql, args)
    }
}
