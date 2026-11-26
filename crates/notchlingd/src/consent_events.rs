use crate::late_emit::Events;
use notchling_engine::consent::{
    AnswerOn, Consent, ACCOUNT, COST_PROFILE, INSTALL, MACHINE, MODEL, SECRET_REQUEST, SESSION,
    SESSION_STOP, SETTING, SWITCH, UPDATE,
};
use notchling_events::field::{Count, Millis};
use notchling_events::field_ref::{Account, Lane, RefKind};
use notchling_events::kinds::agents::AgentConsent;
use notchling_events::kinds::choices::{
    AgentTool, Answerer, ConsentAction, ConsentAnswerOn, ConsentKind, ConsentThen, SwitchName,
};
use notchling_events::kinds::settings::SettingKey;
use notchling_events::spec::Outcome;
use notchling_events::{Event, Ref};

fn kind_of(kind: &str) -> ConsentKind {
    ConsentKind::from_name(kind).unwrap_or(ConsentKind::Other)
}

fn opaque<K: RefKind>(value: &serde_json::Value, prefix: &str) -> Option<Ref<K>> {
    value
        .as_str()
        .filter(|id| id.starts_with(prefix))
        .map(Ref::new)
}

fn account_parts(
    row: &Consent,
) -> (
    Option<Ref<Account>>,
    Option<Ref<Lane>>,
    Option<ConsentAction>,
) {
    if row.kind != ACCOUNT {
        return (None, None, None);
    }
    let change = &row.change;
    let account = opaque(&change["account"], "acct-")
        .or_else(|| opaque(&change["to"], "acct-"))
        .or_else(|| {
            row.result
                .as_ref()
                .and_then(|r| opaque(&r["account"], "acct-"))
        });
    let lane = opaque(&change["lane"], "lane-").or_else(|| opaque(&change["closeLane"], "lane-"));
    let action = match change["action"].as_str() {
        Some("switch") => Some(ConsentAction::AccountSwitch),
        Some("add") => Some(ConsentAction::AccountAdd),
        Some("remove") => Some(ConsentAction::AccountRemove),
        Some("move_all") => Some(ConsentAction::AccountMoveAll),
        Some("move") => Some(ConsentAction::SessionMove),
        Some("lane_create") => Some(ConsentAction::LaneCreate),
        Some("lane_remove") => Some(ConsentAction::LaneRemove),
        Some("lane_switch") => Some(ConsentAction::LaneSwitch),
        _ => None,
    };
    (account, lane, action)
}

fn setup_action(row: &Consent) -> Option<ConsentAction> {
    match row.kind.as_str() {
        UPDATE => Some(ConsentAction::ToolUpdate),
        MODEL => Some(ConsentAction::ModelDownload),
        INSTALL => Some(ConsentAction::InstallSetUp),
        MACHINE => match row.change["action"].as_str() {
            Some("add" | "import") => Some(ConsentAction::MachineAdd),
            Some("remove") => Some(ConsentAction::MachineRemove),
            Some("fix" | "setup" | "running") => Some(ConsentAction::MachineFix),
            Some("addresses" | "update") => Some(ConsentAction::MachineUpdate),
            Some("network") => Some(ConsentAction::TailscaleNetwork),
            _ => None,
        },
        COST_PROFILE => match row.change["action"].as_str() {
            Some("profile") => Some(ConsentAction::CostSet),
            Some("apply") => Some(ConsentAction::CostApply),
            Some("undo") => Some(ConsentAction::CostUndo),
            _ => None,
        },
        _ => None,
    }
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
    let session = (matches!(row.kind.as_str(), SESSION | SESSION_STOP)
        && row.change["action"] != "start")
        .then(|| Ref::new(&row.target));
    let (account, lane, action) = account_parts(row);
    let action = action.or_else(|| setup_action(row));
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
            session,
            machine: (row.kind == MACHINE)
                .then(|| row.change["machine"].as_str())
                .flatten()
                .map(Ref::new),
            account,
            lane,
            job: None,
            action,
            tool: AgentTool::from_name(&row.tool).unwrap_or(AgentTool::Unknown),
            reason_chars: Count(row.reason.as_ref().map_or(0, |r| r.chars().count() as u64)),
            parts: parts(&row.parts),
            parts_allowed: parts(&row.parts_allowed),
            waited: Millis(u64::try_from(waited).unwrap_or(0)),
            noted: row.note.as_ref().map(|_| true),
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

#[cfg(test)]
#[path = "consent_events_tests.rs"]
mod tests;
