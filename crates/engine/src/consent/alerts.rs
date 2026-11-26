use super::kinds::{
    ACCOUNT, CLUSTER, COST_PROFILE, DISPATCH, INSTALL, JOB, JOB_STOP, MACHINE, MODEL,
    SECRET_REQUEST, SESSION, SESSION_STOP, SETTING, SWITCH, UPDATE,
};
use super::model::Consent;
use crate::inbox::AlertAction::{self, Allow, Cancel, Change, Deny, Open, OpenForm};

const BANNER_SWITCHES: [&str; 3] = ["accounts", "install", "machines"];

pub fn alert_actions(row: &Consent) -> Vec<AlertAction> {
    if !row.verified {
        return vec![Deny, Open];
    }
    match row.kind.as_str() {
        SETTING | SESSION_STOP | JOB_STOP => vec![Allow, Deny, Change],
        SWITCH if BANNER_SWITCHES.contains(&row.target.as_str()) => vec![Allow, Deny, Change],
        SWITCH | SESSION | DISPATCH | JOB | MODEL | UPDATE | ACCOUNT | MACHINE | INSTALL
        | COST_PROFILE | CLUSTER => vec![Deny, Open, Change],
        SECRET_REQUEST => vec![OpenForm, Cancel, Change],
        _ => Vec::new(),
    }
}
