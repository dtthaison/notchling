use crate::transcript::model::ItemKind;
use crate::transcript::text::{between, collapse_pastes, prefix_chars, strip_ansi, trim_newlines};

pub fn model_id_in_confirmation(text: &str) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    for lead in ["Set model to `", "Kept model as `"] {
        let mut from = 0;
        while let Some(i) = text[from..].find(lead) {
            let at = from + i;
            let start = at + lead.len();
            let rest = &text[start..];
            if let Some(end) = rest.find('`').filter(|&e| e > 0) {
                if best.is_none_or(|(b, _)| at < b) {
                    best = Some((at, &rest[..end]));
                }
                break;
            }
            from = start;
        }
    }
    best.and_then(|(_, name)| slug(name))
}

fn slug(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    let slug = lower
        .split([' ', '.'])
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() || slug.contains("default") {
        None
    } else {
        Some(format!("claude-{slug}"))
    }
}

fn clip_output(clean: String) -> String {
    if clean.chars().count() > 6000 {
        format!("{}\n…", prefix_chars(&clean, 6000))
    } else {
        clean
    }
}

fn command_output(text: &str) -> Option<String> {
    let raw = between(
        text,
        "<local-command-stdout>",
        "</local-command-stdout>",
        true,
    )
    .or_else(|| {
        between(
            text,
            "<local-command-stderr>",
            "</local-command-stderr>",
            true,
        )
    })?;
    let clean = strip_ansi(&raw).trim().to_string();
    if clean == "(no content)" {
        return Some(String::new());
    }
    Some(clip_output(clean))
}

fn shell_output(text: &str) -> Option<String> {
    let out = between(text, "<bash-stdout>", "</bash-stdout>", false);
    let err = between(text, "<bash-stderr>", "</bash-stderr>", false);
    if out.is_none() && err.is_none() {
        return None;
    }
    let joined = [out, err]
        .into_iter()
        .flatten()
        .map(|s| trim_newlines(&s).to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    Some(clip_output(strip_ansi(&joined)))
}

pub fn user_item(raw: &str, images: usize) -> Option<ItemKind> {
    let mut text = raw.trim().to_string();
    if text.starts_with("[Request interrupted") {
        return Some(ItemKind::notice("Interrupted", "stop.circle"));
    }
    if let Some(output) = command_output(&text) {
        return if output.is_empty() {
            None
        } else {
            Some(ItemKind::CommandOutput { text: output })
        };
    }
    if let Some(command) = between(&text, "<bash-input>", "</bash-input>", true) {
        return Some(ItemKind::User {
            text: format!("! {command}"),
            images,
        });
    }
    if let Some(output) = shell_output(&text) {
        return if output.is_empty() {
            None
        } else {
            Some(ItemKind::CommandOutput { text: output })
        };
    }
    if let Some(name) = between(&text, "<command-name>", "</command-name>", true) {
        let args = between(&text, "<command-args>", "</command-args>", true).unwrap_or_default();
        let slash = if name.starts_with('/') {
            name
        } else {
            format!("/{name}")
        };
        text = if args.is_empty() {
            slash
        } else {
            format!("{slash} {args}")
        };
    } else if text.starts_with('<') && !text.starts_with("<pasted_content") {
        return None;
    }
    let text = collapse_pastes(&text);
    if text.is_empty() && images == 0 {
        return None;
    }
    Some(ItemKind::User { text, images })
}
