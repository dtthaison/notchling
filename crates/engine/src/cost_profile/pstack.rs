use super::model::Profile;
use super::presets::data;
use super::render::{marker, preset_label};
use super::resolve::{resolve, Available};
use crate::catalog::Tool;

const HEAD: [&str; 2] = [
    "# pstack model configuration. One line per role. Delete a line to fall back to the skill default.",
    "# `inherit-parent` or `auto` as a value: the role runs on the parent chat model (omit Agent `model`). Alias entries in a panel list still count toward its fan-out.",
];

fn known(role: &str) -> bool {
    data().pstack.iter().any(|r| r.role == role)
}

fn unknown_lines(existing: Option<&str>) -> Vec<String> {
    existing
        .into_iter()
        .flat_map(str::lines)
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .filter(|l| {
            l.split_once(':')
                .is_some_and(|(role, _)| !known(role.trim()))
        })
        .map(str::to_string)
        .collect()
}

pub fn render(profile: &Profile, avail: &Available, existing: Option<&str>) -> String {
    let mut out = String::new();
    for line in HEAD {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&format!(
        "# {}\n",
        marker(profile).trim_matches(['<', '>', '!', '-', ' '])
    ));
    out.push_str(&format!(
        "# budget: custom (Notchling {})\n",
        preset_label(profile)
    ));
    for entry in &data().pstack {
        let values: Vec<String> = entry
            .uses
            .iter()
            .filter_map(|role| resolve(profile, *role, Tool::Claude, avail))
            .map(|r| r.text())
            .collect();
        if !values.is_empty() {
            out.push_str(&format!("{}: {}\n", entry.role, values.join(", ")));
        }
    }
    for line in unknown_lines(existing) {
        out.push_str(&line);
        out.push('\n');
    }
    out
}
