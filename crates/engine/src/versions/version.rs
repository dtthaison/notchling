use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Claude,
    Codex,
    Agy,
    Notchling,
}

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::Claude, Tool::Codex, Tool::Agy, Tool::Notchling];

    pub fn title(self) -> &'static str {
        match self {
            Tool::Claude => "Claude Code",
            Tool::Codex => "Codex",
            Tool::Agy => "Antigravity",
            Tool::Notchling => "Notchling",
        }
    }

    pub fn command(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
            Tool::Agy => "agy",
            Tool::Notchling => "notchling",
        }
    }
}

const MAX_LEN: usize = 32;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct Version(String);

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        if s.len() > MAX_LEN || !matches_pattern(s) {
            return None;
        }
        Some(Version(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parts(&self) -> ([u128; 3], Option<&str>) {
        let (core, pre) = match self.0.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (self.0.as_str(), None),
        };
        let mut nums = [0u128; 3];
        for (slot, text) in nums.iter_mut().zip(core.split('.')) {
            *slot = text.parse().unwrap_or(u128::MAX);
        }
        (nums, pre)
    }
}

fn matches_pattern(s: &str) -> bool {
    let (core, pre) = match s.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (s, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    if numbers.len() != 3
        || numbers
            .iter()
            .any(|n| n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    match pre {
        None => true,
        Some(p) => !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.'),
    }
}

impl TryFrom<String> for Version {
    type Error = String;

    fn try_from(s: String) -> Result<Version, String> {
        Version::parse(&s).ok_or_else(|| "not a version".to_string())
    }
}

impl From<Version> for String {
    fn from(v: Version) -> String {
        v.0
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> Ordering {
        let (a, pre_a) = self.parts();
        let (b, pre_b) = other.parts();
        a.cmp(&b)
            .then_with(|| match (pre_a, pre_b) {
                (None, None) => Ordering::Equal,
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (Some(x), Some(y)) => x.cmp(y),
            })
            .then_with(|| self.0.cmp(&other.0))
    }
}
