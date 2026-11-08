//! The pieces' tests.

use std::collections::BTreeMap;

use notchling_engine::machines::engine_plan::{
    AdminNeed, ApprovedStep, PlanId, PlanKind, ShownPlan,
};
use notchling_engine::machines::poller::LastReport;
use notchling_engine::machines::registry::{AdminAccess, Machine, MachineOs};
use notchling_engine::machines::status::{Job, WorkerStatus};
use notchling_engine::machines::step::{CheckResult, CheckState, Detected, FixOutcome, StepId};
use notchling_engine::machines::usage::SecretUsage;

use super::pieces::*;

fn status() -> WorkerStatus {
    WorkerStatus {
        machine: "m1".into(),
        jobs_tonight: 3,
        weekly_pct: Some(41.6),
        current: Some(Job {
            repo: "me/app".into(),
            issue: 12,
            stage: "verify".into(),
            ..Job::default()
        }),
        ..WorkerStatus::default()
    }
}

#[test]
fn chip_words_and_details() {
    let m = Machine::new("m1", "m1.test", "worker");
    let none = MachineChip::make(&m, None, None, 0.0);
    assert_eq!(
        (none.word.as_str(), none.tone),
        ("checking…", Tone::Neutral)
    );
    let ok = LastReport::ok(status(), 10.0);
    let chip = MachineChip::make(&m, Some(&ok), None, 20.0);
    assert_eq!((chip.word.as_str(), chip.tone), ("online", Tone::Good));
    assert_eq!(
        chip.details,
        ["3 jobs tonight", "week 42%", "now me/app #12 · verify"]
    );
    let failed = LastReport::failed("ssh: timed out", 30.0);
    let chip = MachineChip::make(&m, Some(&failed), Some(&ok), 40.0);
    assert_eq!((chip.word.as_str(), chip.tone), ("offline", Tone::Bad));
    assert_eq!(chip.error.as_deref(), Some("ssh: timed out"));
    assert!(chip.details.is_empty());
}

#[test]
fn job_facts_add_what_the_chip_leaves_out() {
    let mut s = status();
    s.paused = Some("host load".into());
    s.health.insert("github".into(), "ok".into());
    s.health.insert("claude".into(), "limit".into());
    s.unrouted = vec!["ios".into()];
    s.outbox_failed = 2;
    let lines = job_facts(&s);
    assert_eq!(
        lines[3..],
        [
            "paused: host load".to_string(),
            "health claude: limit".to_string(),
            "unrouted: ios".to_string(),
            "outbox: 2 failed".to_string(),
        ]
    );
}

#[test]
fn running_switch_turns_off_always_and_on_only_past_the_blocker() {
    let on = RunningSwitch::make(
        Some(&CheckState::Pass("".into())),
        Some("Base tools"),
        false,
    );
    assert_eq!((on.on, on.enabled), (true, true));
    let blocked = RunningSwitch::make(
        Some(&CheckState::Fail("".into())),
        Some("Base tools"),
        false,
    );
    assert_eq!((blocked.on, blocked.enabled), (false, false));
    assert_eq!(blocked.note.as_deref(), Some("waits for Base tools"));
    let free = RunningSwitch::make(Some(&CheckState::Fail("".into())), None, false);
    assert!(free.enabled && !free.on);
    let busy = RunningSwitch::make(Some(&CheckState::Fail("".into())), None, true);
    assert!(!busy.enabled);
    let unknown = RunningSwitch::make(
        Some(&CheckState::CantTell("not checked yet".into())),
        None,
        false,
    );
    assert_eq!(unknown.note.as_deref(), Some("not checked yet"));
    assert!(!RunningSwitch::make(None, None, false).enabled);
}

#[test]
fn log_tail_keeps_the_last_300_lines() {
    let lines: Vec<String> = (0..350).map(|i| i.to_string()).collect();
    let tail = LogTail::make(&lines);
    assert_eq!(tail.shown.len(), 300);
    assert_eq!(tail.shown[0], "50");
    assert_eq!(
        tail.note.as_deref(),
        Some("Showing the last 300 of 350 lines")
    );
    assert_eq!(LogTail::make(&lines[..10]).note, None);
}

#[test]
fn a_fix_that_asks_for_another_round_is_guidance() {
    let g = FixLine::of(&FixOutcome::fail("Claude login needs a look; Set up again"));
    assert!(!g.ok && g.guidance);
    let f = FixLine::of(&FixOutcome::fail("exited 1"));
    assert!(!f.ok && !f.guidance);
    assert!(FixLine::of(&FixOutcome::ok("done")).ok);
}

#[test]
fn draft_check_reads_reachable_and_the_facts() {
    let mut results = BTreeMap::new();
    let detected = Detected {
        os: Some(MachineOs::Linux),
        ..Detected::default()
    };
    results.insert(
        StepId::REACHABLE,
        CheckResult::pass("Linux 6.8").detected(detected),
    );
    let inputs = vec!["m1".to_string(), "m1.test".to_string(), "w".to_string()];
    let check = DraftCheck::from_results(&results, &inputs);
    assert!(check.reachable);
    assert_eq!(check.os, Some(MachineOs::Linux));
    assert_eq!(check.lines[0].title, "Reachable");
    assert_eq!(check.abilities_line, "Linux");
    assert_eq!(check.inputs, inputs);
    let refused = DraftCheck::refused(vec!["Name: x".into()], &inputs);
    assert!(!refused.reachable && refused.problems.len() == 1);
}

fn plan(kind: PlanKind, admin: AdminNeed, steps: Vec<ApprovedStep>) -> ShownPlan {
    ShownPlan {
        id: PlanId(7),
        machine: "m1".into(),
        kind,
        title: "Base tools".into(),
        steps,
        admin,
    }
}

fn step(title: &str, display: &str, waiting: bool) -> ApprovedStep {
    ApprovedStep {
        id: StepId::BASE_TOOLS,
        title: title.into(),
        display: display.into(),
        waiting,
        needs_admin: false,
    }
}

#[test]
fn the_fix_sheet_shows_the_commands_and_asks_for_what_the_plan_needs() {
    let m = Machine::new("m1", "m1.test", "worker");
    let p = FixPrompt::new(
        plan(
            PlanKind::Fix(StepId::BASE_TOOLS),
            AdminNeed::Password,
            vec![step("Base tools", "brew install x", false)],
        ),
        &m,
    );
    assert_eq!(p.text(), "brew install x");
    assert_eq!(p.intro(), "These commands run on worker@m1.test:");
    assert!(p.needs_password() && !p.can_run(false) && p.can_run(true));
    assert_eq!(p.warning(), None);

    let root = FixPrompt::new(
        plan(
            PlanKind::Running(true),
            AdminNeed::RootConfirm,
            vec![step("Turn Running on", "systemctl start", false)],
        ),
        &m,
    );
    assert!(!root.needs_password() && root.can_run(false));
    assert_eq!(
        root.warning().as_deref(),
        Some("Admin steps run as root over SSH on m1.test.")
    );

    let blocked = FixPrompt::new(
        plan(
            PlanKind::Fix(StepId::BASE_TOOLS),
            AdminNeed::Refused("no".into()),
            vec![step("Base tools", "x", false)],
        ),
        &m,
    );
    assert_eq!(blocked.blocked(), Some("no"));
    assert!(!blocked.can_run(true));

    let all = FixPrompt::new(
        plan(
            PlanKind::SetUpAll,
            AdminNeed::None,
            vec![
                step("Base tools", "brew x", false),
                step("Plugins", "sd plugins", true),
            ],
        ),
        &m,
    );
    assert_eq!(
        all.text(),
        "▸ Base tools\nbrew x\n\n▸ Plugins (once the steps above pass)\nsd plugins"
    );
    assert!(all.intro().starts_with("These steps run in order"));
}

#[test]
fn an_admin_ask_from_set_up_all_follows_the_machines_admin_access() {
    let mut linux = Machine::new("v", "v.test", "agent");
    linux.os = Some(MachineOs::Linux);
    linux.set_admin(AdminAccess::RootSsh);
    let mut s = step("Service", "systemctl", false);
    s.needs_admin = true;
    let asking = FixPrompt::asking(&linux, &s);
    assert!(asking.asked && asking.plan.admin == AdminNeed::RootConfirm);
    linux.set_admin(AdminAccess::Sudo);
    assert!(matches!(admin_need(&linux, true), AdminNeed::Refused(_)));
    assert_eq!(admin_need(&linux, false), AdminNeed::None);
    let mac = Machine::new("m1", "m1.test", "worker");
    assert_eq!(admin_need(&mac, true), AdminNeed::Password);
}

#[test]
fn offer_titles_and_secret_row_ids() {
    assert_eq!(
        UpdateOfferRow {
            secret: "s".into(),
            count: 1
        }
        .title(),
        "Update 1 machine"
    );
    assert_eq!(
        UpdateOfferRow {
            secret: "s".into(),
            count: 2
        }
        .title(),
        "Update 2 machines"
    );
    let row = SecretRow {
        usage: SecretUsage {
            name: "claude-worker".into(),
            machine: "m1".into(),
            remote_path: "~/.config/agents/claude.env".into(),
            format: notchling_engine::machines::usage::SecretFormat::Raw,
        },
        text: String::new(),
        can_send: false,
    };
    assert_eq!(row.id(), "claude-worker@~/.config/agents/claude.env");
}
