use super::registry::{Machine, MachineOs};
use super::step::{
    run, Cadence, CheckResult, FixOutcome, FixPlan, Probe, RemoteOutput, SetupStep, StepContext,
    StepFix, StepId,
};
use super::steps_machine::{cut, is_newline, outcome, trim_ws, NOT_DETECTED};

const GIT_SCRIPT: &str = r#"if [ "$(uname -s)" = Darwin ] && ! xcode-select -p >/dev/null 2>&1; then echo clt=missing; exit 1; fi; command -v git >/dev/null 2>&1 && git --version"#;

const LINUX_GIT_SCRIPT: &str = "if command -v apt-get >/dev/null; then apt-get install -y git; elif command -v dnf >/dev/null; then dnf install -y git; else exit 3; fi";

const NO_MANAGER: &str = "install git with the machine's package manager";

pub(super) fn step(m: &Machine) -> SetupStep {
    SetupStep::new(
        StepId::NODE_GIT,
        "Git",
        vec![StepId::REACHABLE],
        Cadence::EveryRefresh,
        Probe::remote(GIT_SCRIPT, parse_git),
        fix(m),
    )
}

fn fix(m: &Machine) -> StepFix {
    match m.os {
        Some(MachineOs::Linux) => StepFix::Run(FixPlan::new(
            format!("as admin:\n{LINUX_GIT_SCRIPT}"),
            true,
            |ctx| {
                if ctx.step.machine.os != Some(MachineOs::Linux) {
                    return FixOutcome::fail(
                        "this machine's OS changed since this step was built; check again first",
                    );
                }
                let r = run::admin(ctx, LINUX_GIT_SCRIPT, 600);
                match r.code {
                    3 => FixOutcome::fail(NO_MANAGER),
                    _ => outcome(&r, "git installed", ctx),
                }
            },
        )),
        None => StepFix::Run(FixPlan::new(NOT_DETECTED, false, |_| {
            FixOutcome::fail(NOT_DETECTED)
        })),
        Some(MachineOs::MacOs) => StepFix::HowTo(format!(
            "On {}, at its screen, run xcode-select --install and accept the Command Line Tools. Then check again.",
            m.name
        )),
    }
}

fn parse_git(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    if out
        .text
        .split(is_newline)
        .map(trim_ws)
        .any(|l| l == "clt=missing")
    {
        return CheckResult::fail("Command Line Tools aren't installed");
    }
    let line = out
        .text
        .split(is_newline)
        .map(trim_ws)
        .find(|l| l.starts_with("git version"));
    match line {
        Some(l) if out.code == 0 => CheckResult::pass(cut(&ctx.mask(l))),
        _ => CheckResult::fail("git isn't installed there"),
    }
}
