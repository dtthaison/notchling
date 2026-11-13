use crate::inbox::{ActionId, Item, State};
use notchling_protocol::generated as wire;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxOutcome {
    pub answered: bool,
    pub state: State,
    pub answer: Option<ActionId>,
}

pub fn name_of<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

pub fn action_from(text: &str) -> Option<ActionId> {
    serde_json::from_value(serde_json::Value::from(text)).ok()
}

pub fn wire_item(item: &Item) -> wire::InboxItem {
    let source = &item.source;
    let has_source = source.node.is_some() || source.session.is_some() || source.job.is_some();
    wire::InboxItem {
        id: item.id.clone(),
        kind: name_of(&item.kind),
        title: item.title.clone(),
        detail: item.detail.clone(),
        source: has_source.then(|| wire::InboxItemSource {
            node: source.node.clone(),
            session: source.session.clone(),
            job: source.job.clone(),
        }),
        created_ms: item.created_ms,
        expires_ms: item.expires_ms,
        closed_ms: item.closed_ms,
        actions: item
            .actions
            .iter()
            .map(|action| wire::InboxAction {
                id: name_of(&action.id),
                label: action.label.clone(),
                primary: action.primary,
            })
            .collect(),
        state: name_of(&item.state),
        answer: item.answer.map(|a| name_of(&a)),
        answered_by: item.answered_by.clone(),
        seen: item.seen,
    }
}

pub fn answer_json(outcome: &InboxOutcome) -> serde_json::Value {
    let mut data = serde_json::json!({
        "answered": outcome.answered,
        "state": name_of(&outcome.state),
    });
    if let Some(answer) = outcome.answer {
        data["answer"] = serde_json::Value::from(name_of(&answer));
    }
    data
}
