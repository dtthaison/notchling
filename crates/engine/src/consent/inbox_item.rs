use super::kinds::{spec, SECRET_REQUEST, SETTING};
use super::model::{AnswerOn, Consent};
use crate::inbox::{Action, ActionId, Item, Kind, Source, State};
use serde_json::{json, Value};

pub const TITLE_CHARS: usize = 80;
pub const DETAIL_CHARS: usize = 400;

pub fn item_id(row: &Consent, raised_ms: i64) -> String {
    format!("consent:{}:{raised_ms}", row.id)
}

pub fn consent_of(item_id: &str) -> Option<&str> {
    let rest = item_id.strip_prefix("consent:")?;
    rest.split(':').next().filter(|id| !id.is_empty())
}

fn cut(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn tool_label(tool: &str) -> &'static str {
    match tool {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "antigravity" => "Antigravity",
        "shell" => "A program",
        _ => "An agent",
    }
}

fn shown(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "unset".to_string(),
        other => other.to_string(),
    }
}

pub fn title_of(row: &Consent) -> String {
    let who = tool_label(&row.tool);
    let text = match row.kind.as_str() {
        SETTING => format!("{who} asks to change {}", row.target),
        SECRET_REQUEST => format!("{who} asks you to add the secret {}", row.target),
        _ => format!("{who} asks for your yes"),
    };
    cut(&text, TITLE_CHARS)
}

pub fn detail_of(row: &Consent) -> String {
    let text = match row.kind.as_str() {
        SETTING => {
            let before = row.change.get("before").map(shown).unwrap_or_default();
            let after = row.change.get("after").map(shown).unwrap_or_default();
            format!("{} is now {before}. It would become {after}.", row.target)
        }
        SECRET_REQUEST => {
            "Open the form on your Mac to add it. Its value never reaches the agent.".to_string()
        }
        _ => String::new(),
    };
    cut(&text, DETAIL_CHARS)
}

fn action(id: ActionId, label: &str, primary: bool) -> Action {
    Action {
        id,
        label: label.to_string(),
        primary,
    }
}

pub fn item(row: &Consent, raised_ms: i64) -> Item {
    let kind = spec(&row.kind).map_or(Kind::Consent, |k| k.item);
    let actions = match row.answer_on {
        AnswerOn::MacOrPhone => vec![
            action(ActionId::Allow, "Allow", true),
            action(ActionId::Deny, "Deny", false),
        ],
        AnswerOn::Mac => vec![
            action(ActionId::Deny, "Deny", false),
            action(ActionId::Open, "Open on Mac", true),
        ],
    };
    let mut open = json!({"consent": row.id});
    if row.kind == SECRET_REQUEST {
        open["vault_request"] = json!(row.id);
    }
    Item {
        id: item_id(row, raised_ms),
        kind,
        title: title_of(row),
        detail: detail_of(row),
        source: Source {
            node: None,
            session: row.session.clone(),
            job: None,
        },
        created_ms: row.created_ms,
        expires_ms: Some(row.expires_ms),
        actions,
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: Some(open),
        closed_ms: None,
    }
}
