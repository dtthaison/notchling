use std::collections::{BTreeMap, BTreeSet};

use notchling_engine::machines::registry::MachineOs;
use notchling_engine::machines::step::{CheckResult, Detected, SetupStep, StepFix, StepId};
use notchling_locale as t;

use super::add_parts::Account;
use super::model::ClusterModel;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanRow {
    pub id: StepId,
    pub title: String,
    pub why: String,
    pub reason: String,
    pub display: String,
    pub admin: bool,
    pub howto: bool,
    pub waiting: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub on: bool,
    pub editable: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanView {
    pub rows: Vec<PlanRow>,
    pub needs_password: bool,
    pub root_confirm: bool,
    pub can_be_master: Choice,
    pub acting: Choice,
    pub blocked: Option<String>,
    pub after: Vec<PlanRow>,
}

pub fn why(id: &StepId) -> &'static str {
    match id.raw() {
        "worker-user" => t::cluster::add_why_worker_user(),
        "stays-awake" => t::cluster::add_why_stays_awake(),
        "node-git" => t::cluster::add_why_git(),
        "node-tailscale" => t::cluster::add_why_tailscale(),
        "node-binary" => t::cluster::add_why_binary(),
        "node-service" => t::cluster::add_why_service(),
        "node-app-closed" => t::cluster::add_why_app_closed(),
        "node-reach" => t::cluster::add_why_reach(),
        "node-joined" => t::cluster::add_why_joined(),
        "claude-code" => t::cluster::add_why_claude(),
        _ => t::cluster::add_why_other(),
    }
}

pub fn master_default(
    os: Option<MachineOs>,
    file_vault: Option<bool>,
    user: &str,
    home: Option<&str>,
    primary: Option<&Account>,
) -> Choice {
    let off = |reason: &str, editable: bool| Choice {
        on: false,
        editable,
        reason: Some(reason.to_owned()),
    };
    if os == Some(MachineOs::Linux) {
        return off(t::cluster::add_master_linux(), false);
    }
    match file_vault {
        Some(true) => {}
        Some(false) => return off(t::cluster::add_master_filevault_off(), false),
        None => return off(t::cluster::add_master_filevault_unknown(), false),
    }
    let Some(primary) = primary else {
        return off(t::cluster::add_master_primary_unknown(), false);
    };
    if home != Some(primary.home.as_str()) || user != primary.user {
        return off(t::cluster::add_master_other_account(), false);
    }
    Choice {
        on: true,
        editable: true,
        reason: None,
    }
}

fn is_howto(step: &SetupStep) -> bool {
    matches!(step.fix, StepFix::HowTo(_))
}

pub struct Listed {
    pub before: Vec<PlanRow>,
    pub after: Vec<PlanRow>,
}

pub fn needed(steps: &[SetupStep]) -> BTreeSet<StepId> {
    let mut out = BTreeSet::new();
    let mut todo = vec![StepId::NODE_JOINED];
    while let Some(id) = todo.pop() {
        let Some(step) = steps.iter().find(|s| s.id == id) else {
            continue;
        };
        for need in &step.needs {
            if out.insert(need.clone()) {
                todo.push(need.clone());
            }
        }
    }
    out
}

pub fn rows(steps: &[SetupStep], results: &BTreeMap<StepId, CheckResult>) -> Listed {
    let joined_at = steps.iter().position(|s| s.id == StepId::NODE_JOINED);
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut last = None;
    for (at, step) in steps.iter().enumerate() {
        let Some(result) = results.get(&step.id) else {
            continue;
        };
        let waiting = result.state.is_waiting();
        let failing = result.state.is_fail();
        let howto = is_howto(step);
        if !(failing || (waiting && !howto)) {
            continue;
        }
        let display = match &step.fix {
            StepFix::Run(plan) => plan.display.clone(),
            StepFix::HowTo(text) => text.clone(),
            StepFix::Toggle { .. } => continue,
        };
        let row = PlanRow {
            id: step.id.clone(),
            title: step.title.clone(),
            why: why(&step.id).to_owned(),
            reason: result.state.text().to_owned(),
            display,
            admin: step.plan().is_some_and(|p| p.needs_admin),
            howto,
            waiting,
        };
        if howto && joined_at.is_some_and(|j| at > j) {
            after.push(row);
        } else if step.id == StepId::NODE_JOINED {
            last = Some(row);
        } else {
            before.push(row);
        }
    }
    before.extend(last);
    Listed { before, after }
}

const MAC_MIN: u32 = 13;

pub fn unsupported(detected: &Detected) -> Option<String> {
    let abilities = detected.abilities.as_ref()?;
    let arch = abilities.arch.as_deref();
    match detected.os? {
        MachineOs::MacOs => {
            let version = abilities.os_version.as_deref();
            let major = version.and_then(|v| v.split('.').next()?.parse::<u32>().ok());
            if let (Some(version), Some(major)) = (version, major) {
                if major < MAC_MIN {
                    return Some(t::cluster::add_unsupported_mac(version));
                }
            }
            arch.filter(|a| !["arm64", "x86_64"].contains(a))
                .map(t::cluster::add_unsupported_arch)
        }
        MachineOs::Linux => arch
            .filter(|a| !["x86_64", "aarch64"].contains(a))
            .map(t::cluster::add_unsupported_arch),
    }
}

pub fn primary_account(model: &ClusterModel, this: &Account) -> Option<Account> {
    let row = model.primary_row()?;
    let host = row
        .versions
        .as_ref()
        .and_then(|v| v.host.as_ref())
        .filter(|h| !h.home.is_empty() && !h.user.is_empty());
    match host {
        Some(host) => Some(Account {
            user: host.user.clone(),
            home: host.home.clone(),
        }),
        None => model.this_is_primary().then(|| this.clone()),
    }
}

pub fn says(text: &str, words: &[&str]) -> bool {
    text.to_lowercase().contains(&words.join(" "))
}
