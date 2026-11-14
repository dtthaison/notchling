use notchling_engine::inbox::{ActionId, Change, Item, Kind, State};
use notchling_events::kinds::choices::{ItemAction, ItemKind};
use notchling_events::kinds::inbox::{InboxBy, InboxClosed, InboxRaised, ItemState};
use notchling_events::spec::Outcome;
use notchling_events::{scope, Event, Millis, Ref, Session};

fn kind_of(kind: Kind) -> ItemKind {
    match kind {
        Kind::SecretApproval => ItemKind::SecretApproval,
        Kind::SecretRequest => ItemKind::SecretRequest,
        Kind::LimitNotice => ItemKind::LimitNotice,
        Kind::ActionFailed => ItemKind::ActionFailed,
        Kind::Notice | Kind::VersionCheckFailed | Kind::VersionDrift => ItemKind::Notice,
        Kind::SettingChange => ItemKind::SettingChange,
    }
}

fn action_of(action: ActionId) -> ItemAction {
    serde_json::to_value(action)
        .ok()
        .and_then(|v| v.as_str().and_then(ItemAction::from_name))
        .unwrap_or(ItemAction::Other)
}

fn session_of(item: &Item) -> Option<Ref<Session>> {
    item.source.session.as_deref().map(Ref::<Session>::new)
}

fn raised(item: &Item) -> Event {
    Event::new(
        &InboxRaised {
            item_kind: kind_of(item.kind),
            item: Ref::new(&item.id),
            session: session_of(item),
        },
        Outcome::Waiting,
        item.created_ms,
    )
}

fn closed(item: &Item) -> Event {
    let (state, outcome) = match item.state {
        State::Answered => (ItemState::Answered, Outcome::Ok),
        State::Expired => (ItemState::Expired, Outcome::Info),
        _ => (ItemState::Withdrawn, Outcome::Info),
    };
    let at = item.closed_ms.unwrap_or(item.created_ms);
    Event::new(
        &InboxClosed {
            item_kind: kind_of(item.kind),
            item: Ref::new(&item.id),
            state,
            action: item.answer.map(action_of),
            by: item.answered_by.as_deref().and_then(InboxBy::from_name),
            device: (item.state == State::Answered)
                .then(|| scope::current().actor.and_then(|a| a.device))
                .flatten(),
            waited_ms: Some(Millis(
                u64::try_from(at.saturating_sub(item.created_ms)).unwrap_or(0),
            )),
        },
        outcome,
        at,
    )
}

pub(crate) fn events_of(changes: &[Change]) -> Vec<Event> {
    changes
        .iter()
        .filter_map(|change| match change {
            Change::Raised(item) if item.state == State::Open => Some(raised(item)),
            Change::Updated(item) if item.state != State::Open => Some(closed(item)),
            _ => None,
        })
        .collect()
}
