use crate::late_emit::Events;
use notchling_engine::consent::{AnswerOn, Consent, SECRET_REQUEST, SETTING, SWITCH};
use notchling_events::field::{Count, Millis};
use notchling_events::kinds::agents::AgentConsent;
use notchling_events::kinds::choices::{
    AgentTool, Answerer, ConsentAnswerOn, ConsentKind, ConsentThen, SwitchName,
};
use notchling_events::kinds::settings::SettingKey;
use notchling_events::spec::Outcome;
use notchling_events::{Event, Ref};

fn kind_of(kind: &str) -> ConsentKind {
    ConsentKind::from_name(kind).unwrap_or(ConsentKind::Other)
}

fn outcome_of(then: ConsentThen) -> Outcome {
    match then {
        ConsentThen::Asked | ConsentThen::Restored | ConsentThen::Opened => Outcome::Waiting,
        ConsentThen::Applied | ConsentThen::Allowed | ConsentThen::HandedOver => Outcome::Ok,
        ConsentThen::Refused => Outcome::Refused,
        ConsentThen::Failed => Outcome::Failed,
        _ => Outcome::Info,
    }
}

pub(crate) fn event_of(row: &Consent, then: ConsentThen, now_ms: i64) -> Event {
    let waited = now_ms.saturating_sub(row.created_ms).max(0);
    let by = row.by.as_deref().and_then(Answerer::from_name);
    let (key, secret) = match row.kind.as_str() {
        SETTING => (SettingKey::from_name(&row.target), None),
        SECRET_REQUEST => (None, Some(Ref::new(&row.target))),
        _ => (None, None),
    };
    let parts = |value: &Option<serde_json::Value>| {
        Count(
            value
                .as_ref()
                .and_then(|v| v.as_array())
                .map_or(0, |a| a.len() as u64),
        )
    };
    Event::new(
        &AgentConsent {
            consent: Ref::new(&row.id),
            then,
            kind: kind_of(&row.kind),
            by,
            answer_on: match row.answer_on {
                AnswerOn::Mac => ConsentAnswerOn::Mac,
                AnswerOn::MacOrPhone => ConsentAnswerOn::MacOrPhone,
            },
            switch: (row.kind == SWITCH)
                .then(|| SwitchName::from_name(&row.target).unwrap_or(SwitchName::Other)),
            key,
            secret,
            session: None,
            machine: None,
            account: None,
            lane: None,
            job: None,
            action: None,
            tool: AgentTool::from_name(&row.tool).unwrap_or(AgentTool::Unknown),
            reason_chars: Count(row.reason.as_ref().map_or(0, |r| r.chars().count() as u64)),
            parts: parts(&row.parts),
            parts_allowed: parts(&row.parts_allowed),
            waited: Millis(u64::try_from(waited).unwrap_or(0)),
        },
        outcome_of(then),
        now_ms,
    )
}

pub(crate) fn emit(events: Option<&Events>, row: &Consent, then: ConsentThen, now_ms: i64) {
    if let Some(events) = events {
        events.push(event_of(row, then, now_ms));
    }
}

pub(crate) fn commit(
    events: Option<&Events>,
    row: &Consent,
    then: ConsentThen,
    now_ms: i64,
) -> bool {
    match events {
        Some(events) => events.commit(event_of(row, then, now_ms)),
        None => true,
    }
}
