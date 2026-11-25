use super::registry::Machine;
use super::step::{Cadence, CheckResult, ClusterTokens, Probe, SetupStep, StepFix, StepId};
use crate::cluster::ready::CheckKind;

const IDS: [(CheckKind, StepId); 4] = [
    (CheckKind::Encrypted, StepId::MASTER_ENCRYPTED),
    (CheckKind::SameHome, StepId::MASTER_SAME_HOME),
    (CheckKind::Space, StepId::MASTER_SPACE),
    (CheckKind::Tools, StepId::MASTER_TOOLS),
];

pub fn is_master_step(id: &StepId) -> bool {
    IDS.iter().any(|(_, known)| known == id)
}

pub fn steps(m: &Machine) -> Vec<SetupStep> {
    vec![
        step(
            CheckKind::Encrypted,
            StepId::MASTER_ENCRYPTED,
            "Disk encryption",
            encrypted_howto(m),
        ),
        step(
            CheckKind::SameHome,
            StepId::MASTER_SAME_HOME,
            "Same user and home",
            format!(
                "Use the same user name and home folder on {} as on the primary. A set restores only at identical paths, so Notchling never rewrites them.",
                m.name
            ),
        ),
        step(
            CheckKind::Space,
            StepId::MASTER_SPACE,
            "Space for sets",
            format!(
                "Free some disk space on {}. It needs room for twice the newest set and at least 5 GB.",
                m.name
            ),
        ),
        step(
            CheckKind::Tools,
            StepId::MASTER_TOOLS,
            "Same tool versions",
            format!(
                "Update Claude Code and Codex on {} to the versions the primary runs; Tool Versions lists both.",
                m.name
            ),
        ),
    ]
}

fn encrypted_howto(m: &Machine) -> String {
    if m.is_linux() {
        format!(
            "Put the home folder on {} on an encrypted LUKS volume. Notchling only reads whether it is; it never changes the disk.",
            m.name
        )
    } else {
        format!(
            "Turn on FileVault on {}: System Settings, Privacy & Security, FileVault, Turn On. Notchling only reads whether it is on.",
            m.name
        )
    }
}

fn step(kind: CheckKind, id: StepId, title: &str, howto: String) -> SetupStep {
    SetupStep::new(
        id,
        title,
        vec![StepId::NODE_JOINED],
        Cadence::EveryRefresh,
        Probe::local(move |ctx| {
            let Some(checks) = ctx.tokens.master_checks(ctx.node.as_deref()) else {
                return CheckResult::cant_tell("the cluster hasn't reported this machine yet");
            };
            match checks.into_iter().find(|(k, _)| *k == kind) {
                Some((_, Ok(text))) => CheckResult::pass(text),
                Some((_, Err(text))) => CheckResult::fail(text),
                None => CheckResult::cant_tell("the cluster hasn't reported this check yet"),
            }
        }),
        StepFix::HowTo(howto),
    )
}

pub fn only_masters(
    list: Vec<SetupStep>,
    tokens: &dyn ClusterTokens,
    node: Option<&str>,
) -> Vec<SetupStep> {
    if !list.iter().any(|s| is_master_step(&s.id)) || tokens.master_checks(node).is_some() {
        return list;
    }
    list.into_iter()
        .filter(|s| !is_master_step(&s.id))
        .collect()
}
