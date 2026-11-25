use super::*;
use crate::commands::{rows, Row};
use crate::skills_family::FAMILIES;
use crate::skills_scan::{check_against_rows, commands_named, installed_specs, known};
use notchling_engine::delegate::skill::SkillTool;
use notchling_engine::install::skills::{rendered, SkillSpec, CONTRACT};
use std::collections::BTreeSet;
use std::path::Path;

const BINARY: &str = "/Apps/N";
const ALL_TOOLS: [SkillTool; 3] = [SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity];

fn all_specs() -> Vec<SkillSpec> {
    installed_specs(Path::new(BINARY), Path::new("/nowhere"), None)
}

fn index() -> String {
    let all: Vec<&Row> = rows().collect();
    index_text(BINARY, &all)
}

#[test]
fn every_row_names_an_installed_skill() {
    let names: BTreeSet<String> = all_specs().into_iter().map(|s| s.name).collect();
    for row in rows() {
        assert!(!row.skill.is_empty(), "{} names no skill", row.command);
        assert!(
            names.contains(row.skill),
            "{} names {}, which is not installed",
            row.command,
            row.skill
        );
    }
}

#[test]
fn every_family_has_a_skill_and_a_row() {
    let names: BTreeSet<String> = all_specs().into_iter().map(|s| s.name).collect();
    for family in FAMILIES {
        assert!(names.contains(family.skill), "{}", family.skill);
        assert!(
            rows().any(|r| r.skill == family.skill),
            "{} owns no row",
            family.skill
        );
    }
    assert_eq!(names.len(), FAMILIES.len());
}

#[test]
fn the_index_names_every_family_and_stays_short() {
    let text = index();
    assert!(text.lines().count() <= 25, "{}", text.lines().count());
    for family in FAMILIES {
        assert!(
            text.contains(&format!("`{}`", family.skill)),
            "{}",
            family.skill
        );
    }
    assert!(text.contains("help --json"));
    assert!(text.contains(BINARY));
    assert!(text.contains("service or host"));
}

#[test]
fn every_notchling_skill_ends_with_the_index_and_the_contract() {
    let tail = format!("{}\n{CONTRACT}", index().trim_end());
    let mut seen = 0;
    for spec in all_specs() {
        if !(spec.name == "notchling" || spec.name.starts_with("notchling-")) {
            continue;
        }
        for tool in spec.tools.clone() {
            let text = rendered(&spec, tool);
            assert!(text.ends_with(&tail), "{} for {tool:?}", spec.name);
            assert_eq!(text.matches("## All Notchling commands").count(), 1);
            seen += 1;
        }
    }
    assert!(seen >= FAMILIES.len());
}

#[test]
fn the_notchling_skill_owns_the_rows_no_family_covers() {
    let spec = all_specs()
        .into_iter()
        .find(|s| s.name == "notchling")
        .expect("the notchling skill");
    assert_eq!(spec.tools, ALL_TOOLS.to_vec());
    let text = (spec.text)(SkillTool::Claude);
    assert!(text.starts_with("---\nname: notchling\ndescription: Use "));
    for command in [
        "inbox", "consent", "watch", "switches", "activity", "orgs", "versions",
    ] {
        assert!(text.contains(&format!("'{BINARY}' {command}")), "{command}");
    }
}

#[test]
fn a_family_skill_has_its_description_its_usage_and_help() {
    for name in [
        "notchling-sessions",
        "notchling-jobs",
        "notchling-cluster",
        "notchling-machines",
        "notchling-accounts",
    ] {
        let spec = all_specs()
            .into_iter()
            .find(|s| s.name == name)
            .expect(name);
        for tool in ALL_TOOLS {
            let text = (spec.text)(tool);
            assert!(
                text.starts_with(&format!("---\nname: {name}\ndescription: Use ")),
                "{name}"
            );
            assert!(text.contains(&format!("'{BINARY}' help --json")), "{name}");
            assert!(!text.contains("{binary}"), "{name}");
        }
        assert!(
            !crate::skills_family::usage_lines(name).is_empty(),
            "{name}"
        );
    }
}

#[test]
fn the_rules_are_in_the_family_texts_that_ask_the_user() {
    let spec = all_specs()
        .into_iter()
        .find(|s| s.name == "notchling-cluster")
        .unwrap();
    let text = (spec.text)(SkillTool::Claude);
    assert!(text.contains(crate::skills_family::REASON_RULE));
    assert!(text.contains(crate::skills_family::KEY_RULE));
}

#[test]
fn every_command_a_skill_names_exists() {
    let known = known();
    let mut checked = 0;
    for spec in all_specs() {
        for tool in spec.tools.clone() {
            let text = rendered(&spec, tool);
            for line in commands_named(BINARY, &text) {
                if let Err(why) = check_against_rows(&known, &line) {
                    panic!("{} for {tool:?}: {why}", spec.name);
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "{checked}");
}

#[test]
fn a_scan_finds_a_made_up_command_and_flag() {
    let known = known();
    assert!(check_against_rows(&known, "cluster create x").is_ok());
    assert!(check_against_rows(&known, "cluster teleport x").is_err());
    assert!(check_against_rows(&known, "frobnicate").is_err());
    assert!(check_against_rows(&known, "cluster create x --nope").is_err());
}

#[test]
fn settings_skill_wait_matches_the_timeout() {
    let spec = all_specs()
        .into_iter()
        .find(|s| s.name == "notchling-settings")
        .unwrap();
    let minutes = crate::settings_cli::DEFAULT_TIMEOUT.as_secs() / 60;
    for tool in [SkillTool::Claude, SkillTool::Codex] {
        let text = (spec.text)(tool);
        assert!(
            text.contains(&format!("waits up to {minutes} minutes")),
            "{tool:?}"
        );
    }
}
