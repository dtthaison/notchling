//! What a user line (or a logged slash command) shows as in Chat.

use crate::transcript::model::ItemKind;
use crate::transcript::text::{between, collapse_pastes, prefix_chars, strip_ansi, trim_newlines};

/// The model id from /model's confirmation: "Set model to `Sonnet 5.5` and saved as ..." or
/// "Kept model as `Opus 5.5`" become "claude-sonnet-5-5". None for "Default" or other text.
pub fn model_id_in_confirmation(text: &str) -> Option<String> {
    // The leftmost match of either lead, like the regex the Swift app used.
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

/// A command's printed output without terminal colors; "" when it printed nothing worth showing,
/// None when the text isn't command output.
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

/// What a "!" command printed: its stdout, then its stderr.
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

/// What a user line shows as: the text, a slash command as typed, an interruption, or nothing for
/// injected wrappers.
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
    // A "!" shell command typed at the prompt (or sent from Chat), and what it printed.
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
    // A slash command shows as what was typed: "/name args".
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
        // Injected wrappers: system reminders, command output, hook context.
        return None;
    }
    let text = collapse_pastes(&text);
    if text.is_empty() && images == 0 {
        return None;
    }
    Some(ItemKind::User { text, images })
}
