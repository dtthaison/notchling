use super::model::{Cell, Imported, Profile, ReportLine, Role, CLAUDE_EFFORTS, CLAUDE_FAMILIES};
use super::presets::data;
use crate::catalog::Tool;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub picks: Vec<(Role, Cell)>,
    pub kept: Vec<String>,
    pub report: Vec<ReportLine>,
}

const HEADER_WORDS: [&str; 4] = ["model", "price in/out per 1m tokens", "effort", "use for"];

const KNOWN_USES: [&str; 4] = [
    "bulk reading, search, listing, log scanning, running a check and reporting the result",
    "routine code: specified edits, tests, small features, mechanical refactors, git history digs",
    "judgment: design, review, root-causing, prose, synthesis of other agents' reports",
    "the hardest work only: cross-cutting design, subtle concurrency or algorithms, a bug two fixes failed to kill. turns run long",
];

fn cells_of(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_separator(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn is_row(line: &str) -> bool {
    line.trim().starts_with('|')
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

fn family_cell(cell: &str) -> Option<&'static str> {
    let cell = cell.trim();
    let (word, rest) = match cell.split_once(' ') {
        Some((w, r)) => (w, r.trim()),
        None => (cell, ""),
    };
    let family = CLAUDE_FAMILIES
        .iter()
        .find(|f| **f == word.to_lowercase())
        .copied()?;
    if rest.is_empty() {
        return Some(family);
    }
    let inner = rest.strip_prefix('(')?.strip_suffix(')')?;
    let mut parts = inner.split_whitespace();
    let (name, version) = (parts.next()?, parts.next()?);
    let named = name.chars().all(|c| c.is_ascii_alphabetic());
    let numbered = version.chars().all(|c| c.is_ascii_digit() || c == '.');
    (parts.next().is_none() && named && numbered).then_some(family)
}

fn price_cell(cell: &str) -> bool {
    let money = |s: &str| {
        s.trim()
            .strip_prefix('$')
            .is_some_and(|n| n.parse::<f64>().is_ok())
    };
    cell.split_once('/')
        .is_some_and(|(a, b)| money(a) && money(b))
}

fn known_use(cell: &str) -> bool {
    let norm = cell.trim().trim_end_matches('.').to_lowercase();
    KNOWN_USES.contains(&norm.as_str())
}

fn table_picks(block: &[&str], profile: &Profile) -> Option<Vec<(Role, Cell)>> {
    if block.len() < 3 || !is_separator(block[1]) {
        return None;
    }
    let head: Vec<String> = cells_of(block[0])
        .iter()
        .map(|c| c.to_lowercase())
        .collect();
    let understood = head.iter().all(|c| HEADER_WORDS.contains(&c.as_str()))
        && head.first().map(String::as_str) == Some("model")
        && head.last().map(String::as_str) == Some("use for");
    if !understood {
        return None;
    }
    let mut picks = Vec::new();
    for line in &block[2..] {
        let cells = cells_of(line);
        if cells.len() != head.len() {
            return None;
        }
        let family = family_cell(&cells[0])?;
        let mut effort = None;
        for (name, cell) in head.iter().zip(&cells).skip(1) {
            match name.as_str() {
                "price in/out per 1m tokens" if price_cell(cell) => {}
                "effort" if CLAUDE_EFFORTS.contains(&cell.as_str()) => effort = Some(cell.clone()),
                "use for" if known_use(cell) => {}
                _ => return None,
            }
        }
        let role = role_of_words(cells.last()?)?;
        let effort = effort.unwrap_or_else(|| {
            profile
                .cell(role, Tool::Claude)
                .map(|c| c.effort)
                .unwrap_or_else(|| "high".to_string())
        });
        picks.push((role, Cell::new(family, &effort)));
    }
    Some(picks)
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
    let mut taken: Vec<Role> = Vec::new();
    let say = |number: usize, line: &str, fate: &str, report: &mut Vec<ReportLine>| {
        report.push(ReportLine {
            number,
            text: line.to_string(),
            fate: fate.to_string(),
        })
    };
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if line.trim().is_empty() {
            kept.push(String::new());
            at += 1;
            continue;
        }
        if is_row(line) {
            let end = (at..lines.len())
                .find(|i| !is_row(lines[*i]))
                .unwrap_or(lines.len());
            let block = &lines[at..end];
            let picks = table_picks(block, profile).filter(|p| {
                let mut seen = taken.clone();
                p.iter().all(|(role, _)| {
                    let fresh = !seen.contains(role);
                    seen.push(*role);
                    fresh
                })
            });
            match picks {
                Some(picks) => {
                    for (i, row) in block.iter().enumerate() {
                        if i < 2 {
                            say(at + i + 1, row, "regenerated", &mut out.report);
                        } else {
                            let role = picks[i - 2].0;
                            say(
                                at + i + 1,
                                row,
                                &format!("profile:{}", role.name()),
                                &mut out.report,
                            );
                        }
                    }
                    for (role, cell) in picks {
                        taken.push(role);
                        out.picks.push((role, cell));
                    }
                }
                None => {
                    for (i, row) in block.iter().enumerate() {
                        kept.push(row.to_string());
                        say(at + i + 1, row, "kept", &mut out.report);
                    }
                }
            }
            at = end;
            continue;
        }
        match role_line(line).filter(|(role, _)| !taken.contains(role)) {
            Some((role, cell)) => {
                taken.push(role);
                out.picks.push((role, cell));
                say(
                    at + 1,
                    line,
                    &format!("profile:{}", role.name()),
                    &mut out.report,
                );
            }
            None => {
                kept.push(line.to_string());
                say(at + 1, line, "kept", &mut out.report);
            }
        }
        at += 1;
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
    let imported = Imported {
        source: source.to_string(),
        kept: outcome.kept.clone(),
        report: outcome.report.clone(),
    };
    match profile.imports.iter_mut().find(|i| i.source == source) {
        Some(slot) => *slot = imported,
        None => profile.imports.push(imported),
    }
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
