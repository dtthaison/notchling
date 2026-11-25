use super::model::{Asker, Row, CHOICE_LABEL_CHARS, DETAIL_CHARS, MAX_CHOICES, TITLE_CHARS};
use super::rules::cut;
use crate::inbox::{Action, ActionId, Item, Kind, Source, State};

const CHOICE_ACTIONS: [ActionId; MAX_CHOICES] = [
    ActionId::Choice1,
    ActionId::Choice2,
    ActionId::Choice3,
    ActionId::Choice4,
    ActionId::Choice5,
    ActionId::Choice6,
];

pub fn action_for(choice: u8) -> ActionId {
    let at = usize::from(choice).saturating_sub(1);
    CHOICE_ACTIONS[at.min(MAX_CHOICES - 1)]
}

pub fn choice_of(action: ActionId) -> Option<u8> {
    CHOICE_ACTIONS
        .iter()
        .position(|a| *a == action)
        .map(|at| at as u8 + 1)
}

pub fn job_label(tool: Asker, delegator: Option<&str>) -> String {
    let base = if tool == Asker::Antigravity {
        "Antigravity job"
    } else {
        "Codex job"
    };
    match delegator {
        Some(by) => format!("{base} · delegated by «{by}»"),
        None => base.to_string(),
    }
}

fn label(row: &Row) -> String {
    let named = |fallback: &str| row.label.clone().unwrap_or_else(|| fallback.to_string());
    match row.asker {
        Asker::Unknown => "This session or one of its subagents".to_string(),
        Asker::Main => named("This session"),
        Asker::Subagent => named("A subagent"),
        Asker::Codex => named("Codex job"),
        Asker::Antigravity => named("Antigravity job"),
    }
}

fn title(row: &Row, node: Option<&str>) -> String {
    let mut base = format!("{} asks", label(row));
    if let Some(branch) = &row.branch {
        base.push_str(" · ");
        base.push_str(branch);
    }
    let Some(node) = node else {
        return cut(&base, TITLE_CHARS);
    };
    let suffix = format!(" on {node}");
    let room = TITLE_CHARS.saturating_sub(suffix.chars().count());
    cut(&format!("{}{suffix}", cut(&base, room)), TITLE_CHARS)
}

fn action(id: ActionId, label: &str, primary: bool) -> Action {
    Action {
        id,
        label: label.to_string(),
        primary,
    }
}

pub fn item(row: &Row, node: Option<&str>, now_ms: i64) -> Item {
    let choices = row.choices.as_deref().unwrap_or(&[]);
    let mut actions: Vec<Action> = choices
        .iter()
        .take(MAX_CHOICES)
        .enumerate()
        .map(|(at, text)| {
            action(
                action_for(at as u8 + 1),
                &cut(text, CHOICE_LABEL_CHARS),
                false,
            )
        })
        .collect();
    actions.push(action(ActionId::Open, "Answer in Chat", choices.is_empty()));
    actions.push(action(ActionId::Cancel, "Dismiss", false));
    Item {
        id: format!("ask:{}:{now_ms}", row.id),
        kind: Kind::Question,
        title: title(row, node),
        detail: cut(&row.question, DETAIL_CHARS),
        source: Source {
            node: None,
            session: Some(row.session.clone()),
            job: None,
        },
        created_ms: now_ms,
        expires_ms: None,
        actions,
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: Some(serde_json::json!({"ask": row.id, "session": row.session})),
        closed_ms: None,
        switch: None,
    }
}
