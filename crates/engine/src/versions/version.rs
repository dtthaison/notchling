use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

const MAX_LEN: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Version(String);

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        if s.is_empty() || s.len() > MAX_LEN {
            return None;
        }
        let (core, pre) = match s.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (s, None),
        };
        let mut parts = core.split('.');
        for _ in 0..3 {
            let part = parts.next()?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
        }
        if parts.next().is_some() {
            return None;
        }
        if let Some(pre) = pre {
            if pre.is_empty() || !pre.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.') {
                return None;
            }
        }
        Some(Version(s.to_string()))
    }

    pub fn zero() -> Version {
        Version("0.0.0".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn split(&self) -> ([&str; 3], Option<&str>) {
        let (core, pre) = match self.0.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (self.0.as_str(), None),
        };
        let mut parts = core.split('.');
        let mut numbers = [""; 3];
        for slot in numbers.iter_mut() {
            *slot = parts.next().unwrap_or("0");
        }
        (numbers, pre)
    }
}

fn compare_digits(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

fn compare_identifier(a: &str, b: &str) -> Ordering {
    let a_num = !a.is_empty() && a.bytes().all(|c| c.is_ascii_digit());
    let b_num = !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit());
    match (a_num, b_num) {
        (true, true) => compare_digits(a, b),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.cmp(b),
    }
}

fn compare_prerelease(a: &str, b: &str) -> Ordering {
    let mut left = a.split('.');
    let mut right = b.split('.');
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => match compare_identifier(x, y) {
                Ordering::Equal => {}
                other => return other,
            },
        }
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> Ordering {
        let (a, a_pre) = self.split();
        let (b, b_pre) = other.split();
        for (x, y) in a.iter().zip(b.iter()) {
            match compare_digits(x, y) {
                Ordering::Equal => {}
                other => return other,
            }
        }
        let by_pre = match (a_pre, b_pre) {
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(x), Some(y)) => compare_prerelease(x, y),
        };
        by_pre.then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Version {
    type Error = String;

    fn try_from(value: String) -> Result<Version, String> {
        Version::parse(&value).ok_or_else(|| "not a version".to_string())
    }
}

impl From<Version> for String {
    fn from(value: Version) -> String {
        value.0
    }
}
