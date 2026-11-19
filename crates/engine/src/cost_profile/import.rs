use super::model::{Cell, Profile, ReportLine, Role, CLAUDE_EFFORTS, CLAUDE_FAMILIES};
use super::presets::data;
use crate::catalog::Tool;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub picks: Vec<(Role, Cell)>,
    pub kept: Vec<String>,
    pub report: Vec<ReportLine>,
}

fn cells_of(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_separator(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn role_of_words(text: &str) -> Option<Role> {
    let lower = text.to_lowercase();
    data()
        .import_words
        .iter()
        .find(|(_, words)| words.iter().any(|w| lower.contains(w.as_str())))
        .map(|(role, _)| *role)
}

fn role_of_label(label: &str) -> Option<Role> {
    let l = label.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| l.contains(w));
    if has(&["hardest"]) {
        Some(Role::Hardest)
    } else if has(&["bug", "fix"]) {
        Some(Role::Fix)
    } else if has(&["feature", "refactor", "code", "build"]) {
        Some(Role::Code)
    } else if has(&["judg", "prose", "design", "synthes", "root"]) {
        Some(Role::Judge)
    } else if has(&["read", "explor", "search"]) {
        Some(Role::Read)
    } else {
        None
    }
}

fn family_of(cell: &str) -> Option<&'static str> {
    let word = cell
        .split(|c: char| !c.is_ascii_alphabetic())
        .find(|w| !w.is_empty())?
        .to_lowercase();
    CLAUDE_FAMILIES.iter().find(|f| **f == word).copied()
}

fn role_line(line: &str) -> Option<(Role, Cell)> {
    let (label, value) = line.trim().split_once(':')?;
    if !label
        .chars()
        .all(|c| c.is_ascii_lowercase() || matches!(c, ' ' | ',' | '/' | '-' | '&'))
    {
        return None;
    }
    let mut words = value.split_whitespace();
    let (family, effort) = (words.next()?, words.next()?);
    if words.next().is_some() || !CLAUDE_EFFORTS.contains(&effort) {
        return None;
    }
    let role = role_of_label(label)?;
    Some((
        role,
        Cell::parse(Tool::Claude, &format!("{family} {effort}"))?,
    ))
}

fn tidy(lines: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let blank = line.trim().is_empty();
        if blank && out.last().is_none_or(|l| l.trim().is_empty()) {
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out
}

pub fn import_rules(text: &str, profile: &Profile) -> Outcome {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Outcome::default();
    let mut kept: Vec<String> = Vec::new();
    let mut table_head = false;
    for (at, line) in lines.iter().enumerate() {
        let number = at + 1;
        let say = |fate: &str, report: &mut Vec<ReportLine>| {
            report.push(ReportLine {
                number,
                text: line.to_string(),
                fate: fate.to_string(),
            })
        };
        if line.trim().is_empty() {
            kept.push(String::new());
            continue;
        }
        let next_is_separator = lines.get(at + 1).is_some_and(|n| is_separator(n));
        if is_separator(line) || (line.trim().starts_with('|') && next_is_separator) {
            table_head = true;
            say("regenerated", &mut out.report);
            continue;
        }
        if line.trim().starts_with('|') && table_head {
            let cells = cells_of(line);
            let family = cells.first().and_then(|c| family_of(c));
            let role = role_of_words(&cells.iter().skip(1).cloned().collect::<Vec<_>>().join(" "));
            if let (Some(family), Some(role)) = (family, role) {
                let effort = profile
                    .cell(role, Tool::Claude)
                    .map(|c| c.effort)
                    .unwrap_or_else(|| "high".to_string());
                out.picks.push((role, Cell::new(family, &effort)));
                say(&format!("profile:{}", role.name()), &mut out.report);
                continue;
            }
        } else {
            table_head = false;
        }
        if let Some((role, cell)) = role_line(line) {
            out.picks.push((role, cell));
            say(&format!("profile:{}", role.name()), &mut out.report);
            continue;
        }
        kept.push(line.to_string());
        say("kept", &mut out.report);
    }
    out.kept = tidy(kept);
    out
}

pub fn apply_to(profile: &mut Profile, outcome: &Outcome, source: &str) {
    for (role, cell) in &outcome.picks {
        let differs = profile.cell(*role, Tool::Claude).is_none_or(|c| c != *cell);
        if differs {
            profile.picks.insert((*role, Tool::Claude), cell.clone());
        }
    }
    profile.imported = Some(super::model::Imported {
        source: source.to_string(),
        kept: outcome.kept.clone(),
        report: outcome.report.clone(),
    });
}

pub fn cost_table_picks(text: &str) -> Vec<(Role, Cell)> {
    let mut out = Vec::new();
    for line in text.lines() {
        if !line.trim().starts_with('|') {
            continue;
        }
        let cells = cells_of(line);
        let Some(role) = cells.first().and_then(|c| Role::parse(c)) else {
            continue;
        };
        let (model, effort) = match cells.as_slice() {
            [_, _, _, model, effort] => (model, effort),
            [_, model, effort] => (model, effort),
            _ => continue,
        };
        if let Some(cell) = Cell::parse(Tool::Claude, &format!("{model} {effort}")) {
            out.push((role, cell));
        }
    }
    out
}
