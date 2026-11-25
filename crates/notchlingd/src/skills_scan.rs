use crate::commands::rows;
use crate::skills_family::alternatives;
use crate::skills_index::with_index;
use notchling_engine::delegate::skill::SkillTool;
use notchling_engine::install::skills::SkillSpec;
use notchling_engine::vault::skill as vault_skill;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const ALL_TOOLS: [SkillTool; 3] = [SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity];

pub fn installed_specs(
    binary: &Path,
    codex_home: &Path,
    snapshot: Option<PathBuf>,
) -> Vec<SkillSpec> {
    let text_binary = binary.to_string_lossy().into_owned();
    let mut specs = crate::install_ops::default_skills_with(binary, codex_home, snapshot);
    specs.push(with_index(
        &text_binary.clone(),
        SkillSpec {
            name: vault_skill::NAME.to_string(),
            tools: ALL_TOOLS.to_vec(),
            text: Box::new(move |tool| {
                let tool = match tool {
                    SkillTool::Claude => vault_skill::SkillTool::Claude,
                    SkillTool::Codex => vault_skill::SkillTool::Codex,
                    SkillTool::Antigravity => vault_skill::SkillTool::Antigravity,
                };
                vault_skill::content(&text_binary, &[], tool)
            }),
        },
    ));
    specs
}

pub fn is_word(token: &str) -> bool {
    let mut chars = token.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn flags_in(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|t| t.starts_with("--") && t.len() > 2)
        .map(str::to_string)
        .collect()
}

pub fn commands_named(binary: &str, text: &str) -> Vec<String> {
    let quoted = format!("'{binary}' ");
    let mut found = Vec::new();
    for line in text.lines() {
        let mut starts: Vec<usize> = line
            .match_indices(&quoted)
            .map(|(i, m)| i + m.len())
            .collect();
        for (i, m) in line.match_indices("notchling ") {
            let before = line[..i].chars().next_back();
            if before
                .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '/' || c == '_'))
            {
                starts.push(i + m.len());
            }
        }
        for start in starts {
            let rest = &line[start..];
            let rest = rest.split('`').next().unwrap_or(rest);
            let rest = rest.split(" -- ").next().unwrap_or(rest);
            found.push(rest.to_string());
        }
    }
    found
}

pub fn sh_examples(binary: &str, text: &str) -> Vec<String> {
    let quoted = format!("'{binary}' ");
    let mut found = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(fence) = trimmed.strip_prefix("```") {
            if inside {
                inside = false;
            } else {
                inside = matches!(fence.trim(), "sh" | "bash" | "shell");
            }
            continue;
        }
        if !inside {
            continue;
        }
        let command = trimmed.strip_prefix("$ ").unwrap_or(trimmed);
        if command.starts_with(&quoted) {
            found.push(command.to_string());
        }
    }
    found
}

pub struct Known {
    pub path: Vec<String>,
    pub flags: BTreeSet<String>,
}

pub fn known() -> Vec<Known> {
    let mut out = Vec::new();
    for row in rows() {
        let (alts, tail) = alternatives(row.usage);
        for alt in alts {
            let path: Vec<String> = alt
                .split_whitespace()
                .skip(1)
                .take_while(|t| is_word(t))
                .map(str::to_string)
                .collect();
            let mut flags = flags_in(alt);
            flags.extend(flags_in(tail));
            out.push(Known { path, flags });
        }
    }
    out
}

pub fn row_path_len(known: &[Known], line: &str) -> Option<usize> {
    let words: Vec<&str> = line.split_whitespace().take_while(|t| is_word(t)).collect();
    known
        .iter()
        .filter(|k| {
            !k.path.is_empty()
                && k.path.len() <= words.len()
                && k.path.iter().zip(&words).all(|(a, b)| a == b)
        })
        .map(|k| k.path.len())
        .max()
}

pub fn check_against_rows(known: &[Known], line: &str) -> Result<(), String> {
    let words: Vec<&str> = line.split_whitespace().take_while(|t| is_word(t)).collect();
    let Some(best) = row_path_len(known, line) else {
        return Err(format!("`{line}` is no command"));
    };
    let allowed: BTreeSet<&String> = known
        .iter()
        .filter(|k| k.path.len() == best && k.path.iter().zip(&words).all(|(a, b)| a == b))
        .flat_map(|k| k.flags.iter())
        .collect();
    for flag in flags_in(line) {
        if !allowed.contains(&flag) {
            return Err(format!("`{line}` uses {flag}, which the command lacks"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "skills_scan_tests.rs"]
mod tests;
