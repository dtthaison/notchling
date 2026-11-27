use super::model::{Action, ActionId, AlertAction, Kind};

pub fn peer_alert_actions(kind: Kind, actions: &[Action]) -> Vec<AlertAction> {
    let has = |id: ActionId| actions.iter().any(|action| action.id == id);
    match kind {
        Kind::Consent if has(ActionId::Deny) && has(ActionId::Allow) => {
            vec![AlertAction::Deny, AlertAction::Open, AlertAction::Change]
        }
        Kind::Consent if has(ActionId::Deny) => vec![AlertAction::Deny, AlertAction::Open],
        Kind::SecretApproval if has(ActionId::Deny) => {
            vec![AlertAction::Deny, AlertAction::Open, AlertAction::Change]
        }
        Kind::SecretRequest => vec![
            AlertAction::OpenForm,
            AlertAction::Cancel,
            AlertAction::Change,
        ],
        Kind::Question => vec![AlertAction::Reply, AlertAction::Dismiss],
        _ => Vec::new(),
    }
}
