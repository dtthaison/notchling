use crate::commands::{rows, Row};
use crate::skills_family::{minimal, FAMILIES, NAMING_RULE};
use crate::{skill_accounts, skill_cluster, skill_jobs, skill_machines, skill_sessions};
use notchling_engine::delegate::skill::SkillTool;
use notchling_engine::install::skills::SkillSpec;

pub const NAME: &str = "notchling";

const DESCRIPTION: &str = "Use for any Notchling command that no other Notchling skill covers (status, the inbox, consent requests, watching events, switches, activity, organizations, versions, costs, install), and to find which skill covers a command.";

const ABOUT: &str = "These commands read what Notchling knows about this session and its organization, or ask the user for a change with a card. Another skill covers each of the other families; the list is at the end of this skill.";

pub fn index_text(binary: &str, all: &[&Row]) -> String {
    let mut text = format!(
        "## All Notchling commands\n\nOne binary, one skill per family. `'{binary}' help --json` lists every command with its flags, exit codes and skill.\n\n"
    );
    for family in FAMILIES {
        let mut words: Vec<&str> = Vec::new();
        for row in all.iter().filter(|r| r.skill == family.skill) {
            let word = row.command.split(' ').next().unwrap_or(row.command);
            if !words.contains(&word) {
                words.push(word);
            }
        }
        text.push_str(&format!(
            "- `{}`: {} ({})\n",
            family.skill,
            family.about,
            words.join(", ")
        ));
    }
    text.push_str(&format!("\n{NAMING_RULE}\n"));
    text
}

pub fn with_index(binary: &str, spec: SkillSpec) -> SkillSpec {
    if !(spec.name == NAME || spec.name.starts_with("notchling-")) {
        return spec;
    }
    let binary = binary.to_string();
    let inner = spec.text;
    SkillSpec {
        name: spec.name,
        tools: spec.tools,
        text: Box::new(move |tool| {
            let all: Vec<&Row> = rows().collect();
            format!("{}\n{}", inner(tool).trim_end(), index_text(&binary, &all))
        }),
    }
}

fn family_spec(binary: &str, name: &str, make: fn(&str) -> String) -> SkillSpec {
    let for_text = binary.to_string();
    with_index(
        binary,
        SkillSpec {
            name: name.to_string(),
            tools: vec![SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity],
            text: Box::new(move |_| make(&for_text)),
        },
    )
}

pub fn content(binary: &str) -> String {
    minimal(NAME, DESCRIPTION, "Notchling", ABOUT, binary)
}

pub fn extra_specs(binary: &str) -> Vec<SkillSpec> {
    vec![
        family_spec(binary, NAME, content),
        family_spec(binary, skill_sessions::NAME, skill_sessions::content),
        family_spec(binary, skill_jobs::NAME, skill_jobs::content),
        family_spec(binary, skill_cluster::NAME, skill_cluster::content),
        family_spec(binary, skill_machines::NAME, skill_machines::content),
        family_spec(binary, skill_accounts::NAME, skill_accounts::content),
    ]
}

#[cfg(test)]
#[path = "skills_index_tests.rs"]
mod tests;
