use notchling_engine::inbox::{ActionId, AlertAction, Item, State};

use super::model::{consent_of, words_of, InboxModel};
use crate::chat::format::age;
use crate::menu::BlockingRow;

#[derive(Clone)]
pub struct Attention {
    pub blocking: Vec<BlockingRow>,
    pub waiting_count: usize,
}

fn is_open_blocking(item: &Item) -> bool {
    item.state == State::Open && item.is_blocking()
}

fn ends_at(item: &Item) -> i64 {
    item.expires_ms.unwrap_or(i64::MAX)
}

fn row_of(item: &Item, now_ms: i64) -> BlockingRow {
    let (title, _) = words_of(item);
    let who = item
        .place
        .as_ref()
        .and_then(|place| {
            place
                .session_name
                .clone()
                .or_else(|| place.machine_name.clone())
        })
        .unwrap_or_default();
    BlockingRow {
        id: item.id.clone(),
        title,
        who,
        waited: age(now_ms, item.waiting_since.unwrap_or(item.created_ms)),
        allow_from_menu: item.alert_actions.contains(&AlertAction::Allow),
        deny_from_menu: deny_route(item).is_some(),
    }
}

pub fn attention(model: &InboxModel, now_ms: i64) -> Attention {
    let mut blocking: Vec<&Item> = model
        .items()
        .iter()
        .filter(|item| is_open_blocking(item))
        .collect();
    blocking.sort_by(|a, b| {
        ends_at(a)
            .cmp(&ends_at(b))
            .then(a.created_ms.cmp(&b.created_ms))
            .then(a.id.cmp(&b.id))
    });
    let waiting_count = model
        .items()
        .iter()
        .filter(|item| item.state == State::Open && !item.is_blocking())
        .count();
    Attention {
        blocking: blocking
            .into_iter()
            .map(|item| row_of(item, now_ms))
            .collect(),
        waiting_count,
    }
}

pub fn consent_id(item: &Item) -> Option<String> {
    item.open.as_ref().and_then(consent_of).map(str::to_owned)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DenyRoute {
    Consent(String),
    Answer(ActionId),
}

pub fn deny_route(item: &Item) -> Option<DenyRoute> {
    let offers = |alert: AlertAction| item.alert_actions.contains(&alert);
    let answers = |id: ActionId| item.actions.iter().any(|action| action.id == id);
    if offers(AlertAction::Deny) {
        if let Some(consent) = consent_id(item) {
            return Some(DenyRoute::Consent(consent));
        }
        if answers(ActionId::Deny) {
            return Some(DenyRoute::Answer(ActionId::Deny));
        }
    }
    if (offers(AlertAction::Dismiss) || offers(AlertAction::Cancel)) && answers(ActionId::Cancel) {
        return Some(DenyRoute::Answer(ActionId::Cancel));
    }
    None
}
