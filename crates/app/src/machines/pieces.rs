//! The window's pure pieces (Swift `MachinesWindow`'s `MachineChip`, `RunningSwitch`, `LogTail`,
//! `FixLine`, `DraftCheck` and `FixPrompt`, and `MachineCards`' `JobFacts`): what a card and a
//! sheet say, worked out from the engine's results. Nothing here runs anything.

use std::collections::BTreeMap;

use notchling_engine::machines::engine_plan::{
    AdminNeed, ApprovedStep, PlanId, PlanKind, ShownPlan,
};
use notchling_engine::machines::forms::{abilities_line, update_offer_title};
use notchling_engine::machines::poller::{health, LastReport};
use notchling_engine::machines::registry::{AdminAccess, Machine, MachineOs};
use notchling_engine::machines::shell::admin_refusal;
use notchling_engine::machines::status::WorkerStatus;
use notchling_engine::machines::step::{CheckResult, CheckState, Detected, FixOutcome, StepId};
use notchling_engine::machines::usage::SecretUsage;

/// The tone of a machine's health word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Good,
    Warn,
    Bad,
}

/// What the Machines header shows for one machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineChip {
    pub name: String,
    pub word: String,
    pub tone: Tone,
    /// "3 jobs tonight", "week 42%", the job in flight; only while the last poll worked.
    pub details: Vec<String>,
    /// Why the last poll failed, when it did.
    pub error: Option<String>,
    pub token_warning: Option<String>,
}

impl MachineChip {
    pub fn make(
        machine: &Machine,
        report: Option<&LastReport>,
        previous: Option<&LastReport>,
        now: f64,
    ) -> MachineChip {
        let Some(report) = report else {
            return MachineChip {
                name: machine.name.clone(),
                word: "checking…".into(),
                tone: Tone::Neutral,
                details: Vec::new(),
                error: None,
                token_warning: None,
            };
        };
        let word = health::word(machine.file_vault, report, previous, now);
        let tone = if word == health::ONLINE {
            Tone::Good
        } else if word == health::OFFLINE {
            Tone::Bad
        } else {
            Tone::Warn
        };
        let (details, token_warning) = match (&report.status, report.succeeded()) {
            (Some(status), true) => (MachineChip::details(status), health::token_warning(status)),
            _ => (Vec::new(), None),
        };
        MachineChip {
            name: machine.name.clone(),
            word: word.into(),
            tone,
            details,
            error: report.error.clone(),
            token_warning,
        }
    }

    /// "3 jobs tonight", "week 42%", "now me/app #12 · verify".
    pub fn details(status: &WorkerStatus) -> Vec<String> {
        let n = status.jobs_tonight;
        let mut details = vec![format!(
            "{n} {} tonight",
            if n == 1 { "job" } else { "jobs" }
        )];
        if let Some(pct) = status.weekly_pct {
            details.push(format!("week {}%", pct.round() as i64));
        }
        if let Some(job) = &status.current {
            let stage = if job.stage.is_empty() {
                String::new()
            } else {
                format!(" · {}", job.stage)
            };
            details.push(format!("now {} #{}{stage}", job.repo, job.issue));
        }
        details
    }
}

/// What a job card shows from the worker's status: the chip's details, then what plan C never
/// showed (paused, health, unrouted, the outbox, TestFlight).
pub fn job_facts(status: &WorkerStatus) -> Vec<String> {
    let mut lines = MachineChip::details(status);
    if let Some(paused) = status.paused.as_ref().filter(|p| !p.is_empty()) {
        lines.push(format!("paused: {paused}"));
    }
    for (key, value) in &status.health {
        if value != "ok" {
            lines.push(format!("health {key}: {value}"));
        }
    }
    if !status.unrouted.is_empty() {
        lines.push(format!("unrouted: {}", status.unrouted.join(", ")));
    }
    if status.outbox_failed > 0 {
        lines.push(format!("outbox: {} failed", status.outbox_failed));
    }
    if status.testflight.as_ref().is_some_and(|t| t.quiet) {
        lines.push("TestFlight: no new feedback in over 7 days".into());
    }
    lines
}

/// What the Running switch shows and lets the user do. Its position and note come from the
/// Running probe alone; the steps before it only decide whether the worker may be turned on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningSwitch {
    pub on: bool,
    pub enabled: bool,
    pub note: Option<String>,
}

impl RunningSwitch {
    /// `state` is the Running step's own result (✓ is on, ✗ is off, can't tell is neither),
    /// `blocker` the first step before Running that isn't ✓ (it blocks turning on only), `busy`
    /// whether a Fix runs on the machine.
    pub fn make(state: Option<&CheckState>, blocker: Option<&str>, busy: bool) -> RunningSwitch {
        match state {
            // off is always possible
            Some(CheckState::Pass(_)) => RunningSwitch {
                on: true,
                enabled: !busy,
                note: None,
            },
            Some(CheckState::Fail(_)) => match blocker {
                Some(b) => RunningSwitch {
                    on: false,
                    enabled: false,
                    note: Some(format!("waits for {b}")),
                },
                None => RunningSwitch {
                    on: false,
                    enabled: !busy,
                    note: None,
                },
            },
            Some(CheckState::CantTell(why)) => RunningSwitch {
                on: false,
                enabled: false,
                note: Some(why.clone()),
            },
            None => RunningSwitch {
                on: false,
                enabled: false,
                note: None,
            },
        }
    }
}

/// What the machine card's log pane shows: the last lines, and a note when the rest is hidden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogTail {
    pub shown: Vec<String>,
    pub note: Option<String>,
}

impl LogTail {
    pub const LIMIT: usize = 300;

    pub fn make(lines: &[String]) -> LogTail {
        if lines.len() <= LogTail::LIMIT {
            return LogTail {
                shown: lines.to_vec(),
                note: None,
            };
        }
        LogTail {
            shown: lines[lines.len() - LogTail::LIMIT..].to_vec(),
            note: Some(format!(
                "Showing the last {} of {} lines",
                LogTail::LIMIT,
                lines.len()
            )),
        }
    }
}

/// The last Fix's outcome on a machine. A step that asks for another round ("Set up again",
/// "Check again", Set up all's "still not ✓") is guidance, not a red failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixLine {
    pub ok: bool,
    pub text: String,
    pub guidance: bool,
}

impl FixLine {
    pub fn of(outcome: &FixOutcome) -> FixLine {
        let another_round = ["Set up again", "Check again", "still not ✓"]
            .iter()
            .any(|w| outcome.message.contains(w));
        FixLine {
            ok: outcome.ok,
            text: outcome.message.clone(),
            guidance: !outcome.ok && another_round,
        }
    }

    pub fn note(text: impl Into<String>) -> FixLine {
        FixLine {
            ok: true,
            text: text.into(),
            guidance: false,
        }
    }

    pub fn fail(text: impl Into<String>) -> FixLine {
        FixLine {
            ok: false,
            text: text.into(),
            guidance: false,
        }
    }
}

/// One line of the Add sheet's Check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftLine {
    pub title: String,
    pub state: CheckState,
}

/// What the Add sheet's Check found about a draft machine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DraftCheck {
    pub reachable: bool,
    pub os: Option<MachineOs>,
    pub lines: Vec<DraftLine>,
    pub abilities_line: String,
    pub problems: Vec<String>,
    pub detected: Detected,
    /// The name, host and user this check was made for. Add takes the check only for the same
    /// three.
    pub inputs: Vec<String>,
}

impl DraftCheck {
    pub fn refused(problems: Vec<String>, inputs: &[String]) -> DraftCheck {
        DraftCheck {
            problems,
            inputs: inputs.to_vec(),
            ..DraftCheck::default()
        }
    }

    /// From the engine's draft check.
    pub fn from_results(results: &BTreeMap<StepId, CheckResult>, inputs: &[String]) -> DraftCheck {
        let mut check = DraftCheck {
            inputs: inputs.to_vec(),
            ..DraftCheck::default()
        };
        if let Some(reachable) = results.get(&StepId::REACHABLE) {
            check.lines = vec![DraftLine {
                title: "Reachable".into(),
                state: reachable.state.clone(),
            }];
            check.reachable = reachable.state.is_pass();
            check.detected = reachable.detected.clone();
        }
        if let Some(fv) = results
            .get(&StepId::FILEVAULT)
            .and_then(|r| r.detected.file_vault)
        {
            check.detected.file_vault = Some(fv);
        }
        check.os = check.detected.os;
        if check.reachable {
            check.abilities_line =
                abilities_line(check.detected.abilities.as_ref(), check.detected.os);
        }
        check
    }
}

/// What an admin step needs on `m`, as the engine decides it for a plan.
pub fn admin_need(m: &Machine, needs: bool) -> AdminNeed {
    if !needs {
        return AdminNeed::None;
    }
    if let Some(why) = admin_refusal(m) {
        return AdminNeed::Refused(why.into());
    }
    match m.admin() {
        AdminAccess::RootSsh => AdminNeed::RootConfirm,
        AdminAccess::Sudo => AdminNeed::Password,
    }
}

/// What the Fix sheet shows: a plan the engine showed, with the machine's address. It never holds
/// a password: the sheet keeps that in its own state and hands it to the one confirm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixPrompt {
    pub plan: ShownPlan,
    pub host: String,
    pub user: String,
    /// Set up all reached this admin step: the answer goes back to the run, not to a new plan.
    pub asked: bool,
}

impl FixPrompt {
    pub fn new(plan: ShownPlan, m: &Machine) -> FixPrompt {
        FixPrompt {
            plan,
            host: m.host.clone(),
            user: m.user.clone(),
            asked: false,
        }
    }

    /// The sheet for an admin step Set up all reached.
    pub fn asking(m: &Machine, step: &ApprovedStep) -> FixPrompt {
        let plan = ShownPlan {
            id: PlanId(0),
            machine: m.name.clone(),
            kind: PlanKind::Fix(step.id.clone()),
            title: step.title.clone(),
            steps: vec![step.clone()],
            admin: admin_need(m, step.needs_admin),
        };
        FixPrompt {
            plan,
            host: m.host.clone(),
            user: m.user.clone(),
            asked: true,
        }
    }

    pub fn title(&self) -> &str {
        &self.plan.title
    }

    /// Why an admin step can't run on this machine at all (Linux, the worker account as admin).
    pub fn blocked(&self) -> Option<&str> {
        match &self.plan.admin {
            AdminNeed::Refused(why) => Some(why),
            _ => None,
        }
    }

    pub fn needs_password(&self) -> bool {
        self.plan.admin == AdminNeed::Password
    }

    pub fn warning(&self) -> Option<String> {
        (self.plan.admin == AdminNeed::RootConfirm)
            .then(|| format!("Admin steps run as root over SSH on {}.", self.host))
    }

    pub fn intro(&self) -> String {
        match &self.plan.kind {
            PlanKind::SetUpAll => {
                "These steps run in order and stop at the first one that fails:".into()
            }
            PlanKind::Update(_) => "These deliveries run, one machine at a time:".into(),
            _ => format!("These commands run on {}@{}:", self.user, self.host),
        }
    }

    /// The exact commands, as the sheet prints them.
    pub fn text(&self) -> String {
        match &self.plan.kind {
            PlanKind::SetUpAll => self
                .plan
                .steps
                .iter()
                .map(|s| {
                    let waiting = if s.waiting {
                        " (once the steps above pass)"
                    } else {
                        ""
                    };
                    format!("▸ {}{waiting}\n{}", s.title, s.display)
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
            PlanKind::Update(_) => self.plan.display(),
            _ => self
                .plan
                .steps
                .first()
                .map(|s| s.display.clone())
                .unwrap_or_default(),
        }
    }

    /// Run is possible: nothing blocks it, and the password is there when one is needed.
    pub fn can_run(&self, password_given: bool) -> bool {
        self.blocked().is_none() && (!self.needs_password() || password_given)
    }
}

/// A secret file of a machine with what the checks found about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretRow {
    pub usage: SecretUsage,
    pub text: String,
    /// Send again is possible: the vault lists the secret.
    pub can_send: bool,
}

impl SecretRow {
    pub fn id(&self) -> String {
        format!("{}@{}", self.usage.name, self.usage.remote_path)
    }
}

/// "Update N machines", one banner per renewed secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateOfferRow {
    pub secret: String,
    pub count: usize,
}

impl UpdateOfferRow {
    pub fn title(&self) -> String {
        update_offer_title(self.count)
    }
}
