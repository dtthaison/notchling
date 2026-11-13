use serde::Serialize;
use std::collections::HashMap;

pub const MAX_FILES: usize = 500;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Change {
    pub path: String,
    pub status: String,
    pub from: Option<String>,
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Status {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub detached: bool,
    pub initial: bool,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub changes: Vec<Change>,
    pub truncated: bool,
}

pub fn letter(xy: &str) -> &'static str {
    let mut chars = xy.chars();
    let (x, y) = (chars.next().unwrap_or('.'), chars.next().unwrap_or('.'));
    if x == 'U' || y == 'U' || xy == "AA" || xy == "DD" {
        return "U";
    }
    for (letters, answer) in [("RC", "R"), ("D", "D"), ("A", "A")] {
        if letters.contains(x) || letters.contains(y) {
            return answer;
        }
    }
    "M"
}

fn header(status: &mut Status, line: &str) {
    let Some((key, value)) = line.split_once(' ') else {
        return;
    };
    match key {
        "branch.oid" => {
            if value == "(initial)" {
                status.initial = true;
            } else {
                status.head = Some(value.chars().take(7).collect());
            }
        }
        "branch.head" => {
            if value == "(detached)" {
                status.detached = true;
            } else {
                status.branch = Some(value.to_owned());
            }
        }
        "branch.upstream" => status.upstream = Some(value.to_owned()),
        "branch.ab" => {
            for part in value.split(' ') {
                if let Some(n) = part.strip_prefix('+') {
                    status.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    status.behind = n.parse().unwrap_or(0);
                }
            }
        }
        _ => {}
    }
}

fn entry(record: &str) -> Option<(Change, bool)> {
    let kind = record.chars().next()?;
    let mut change = Change::default();
    match kind {
        '1' => {
            let fields: Vec<&str> = record.splitn(9, ' ').collect();
            if fields.len() < 9 {
                return None;
            }
            change.status = letter(fields[1]).to_owned();
            change.path = fields[8].to_owned();
            Some((change, false))
        }
        '2' => {
            let fields: Vec<&str> = record.splitn(10, ' ').collect();
            if fields.len() < 10 {
                return None;
            }
            change.status = letter(fields[1]).to_owned();
            change.path = fields[9].to_owned();
            Some((change, true))
        }
        'u' => {
            let fields: Vec<&str> = record.splitn(11, ' ').collect();
            if fields.len() < 11 {
                return None;
            }
            change.status = "U".to_owned();
            change.path = fields[10].to_owned();
            Some((change, false))
        }
        '?' => {
            change.status = "?".to_owned();
            change.path = record.strip_prefix("? ")?.to_owned();
            Some((change, false))
        }
        _ => None,
    }
}

pub fn parse_status(text: &str) -> Status {
    let mut status = Status::default();
    let mut records = text.split('\0');
    while let Some(record) = records.next() {
        if let Some(line) = record.strip_prefix("# ") {
            header(&mut status, line);
            continue;
        }
        let Some((mut change, has_origin)) = entry(record) else {
            continue;
        };
        if has_origin {
            change.from = records.next().filter(|s| !s.is_empty()).map(String::from);
        }
        if status.changes.len() >= MAX_FILES {
            status.truncated = true;
        } else {
            status.changes.push(change);
        }
    }
    status
}

pub fn parse_numstat(text: &str) -> HashMap<String, (Option<u64>, Option<u64>)> {
    let mut counts = HashMap::new();
    let mut records = text.split('\0');
    while let Some(record) = records.next() {
        let mut cols = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (cols.next(), cols.next(), cols.next())
        else {
            continue;
        };
        let path = if path.is_empty() {
            let _old = records.next();
            records.next().unwrap_or("")
        } else {
            path
        };
        if path.is_empty() {
            continue;
        }
        counts.insert(path.to_owned(), (added.parse().ok(), removed.parse().ok()));
    }
    counts
}

impl Status {
    pub fn apply_numstat(&mut self, text: &str) {
        let counts = parse_numstat(text);
        for change in &mut self.changes {
            if let Some((added, removed)) = counts.get(&change.path) {
                change.added = *added;
                change.removed = *removed;
            }
        }
    }
}
