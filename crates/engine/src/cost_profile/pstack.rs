use super::detect::ours;
use super::model::Profile;
use super::presets::data;
use super::render::{marker, preset_label};
use super::resolve::{resolve, Available};
use crate::catalog::Tool;

const HEAD: [&str; 2] = [
    "# pstack model configuration. One line per role. Delete a line to fall back to the skill default.",
    "# `inherit-parent` or `auto` as a value: the role runs on the parent chat model (omit Agent `model`). Alias entries in a panel list still count toward its fan-out.",
];

fn values_of(role: &str, profile: &Profile, avail: &Available) -> Option<String> {
    let entry = data().pstack.iter().find(|r| r.role == role)?;
    let values: Vec<String> = entry
        .uses
        .iter()
        .filter_map(|r| resolve(profile, *r, Tool::Claude, avail))
        .map(|r| r.text())
        .collect();
    (!values.is_empty()).then(|| values.join(", "))
}

fn marker_line(profile: &Profile) -> String {
    format!(
        "# {}",
        marker(profile).trim_matches(['<', '>', '!', '-', ' '])
    )
}

fn budget_line(profile: &Profile) -> String {
    format!("# budget: custom (Notchling {})", preset_label(profile))
}

fn body_of(line: &str) -> (&str, &str) {
    let body = line.trim_end_matches(['\n', '\r']);
    (body, &line[body.len()..])
}

pub fn render(profile: &Profile, avail: &Available, existing: Option<&str>) -> String {
    let Some(existing) = existing.filter(|t| !t.trim().is_empty()) else {
        return fresh(profile, avail);
    };
    let eol = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = String::new();
    let mut seen: Vec<String> = Vec::new();
    let mut marked = false;
    let mut head_done = false;
    for line in existing.split_inclusive('\n') {
        let (body, end) = body_of(line);
        let end = if end.is_empty() { eol } else { end };
        if body.trim_start().starts_with('#') {
            if ours(body) {
                out.push_str(&format!("{}{end}", marker_line(profile)));
                marked = true;
            } else if body.trim_start().starts_with("# budget:") {
                out.push_str(&format!("{}{end}", budget_line(profile)));
            } else {
                out.push_str(&format!("{body}{end}"));
            }
            continue;
        }
        if !head_done && !marked {
            out.push_str(&format!("{}{end}", marker_line(profile)));
            marked = true;
        }
        head_done = true;
        let known = body
            .split_once(':')
            .map(|(label, _)| label.trim())
            .and_then(|label| values_of(label, profile, avail).map(|v| (label.to_string(), v)));
        match known {
            Some((label, values)) => {
                seen.push(label.clone());
                let indent = &body[..body.len() - body.trim_start().len()];
                out.push_str(&format!("{indent}{label}: {values}{end}"));
            }
            None => out.push_str(&format!("{body}{end}")),
        }
    }
    if !out.ends_with('\n') {
        out.push_str(eol);
    }
    if !marked {
        out.push_str(&format!("{}{eol}", marker_line(profile)));
    }
    for entry in &data().pstack {
        if seen.contains(&entry.role) {
            continue;
        }
        if let Some(values) = values_of(&entry.role, profile, avail) {
            out.push_str(&format!("{}: {values}{eol}", entry.role));
        }
    }
    out
}

fn fresh(profile: &Profile, avail: &Available) -> String {
    let mut out = String::new();
    for line in HEAD {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&marker_line(profile));
    out.push('\n');
    out.push_str(&budget_line(profile));
    out.push('\n');
    for entry in &data().pstack {
        if let Some(values) = values_of(&entry.role, profile, avail) {
            out.push_str(&format!("{}: {values}\n", entry.role));
        }
    }
    out
}
