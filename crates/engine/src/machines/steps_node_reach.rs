use super::registry::Machine;
use super::runner::Call;
use super::shell;
use super::step::{Cadence, CheckResult, Probe, SetupStep, StepContext, StepFix, StepId};
use super::steps_node::{first_text_line, machine_refusal, BINARY};
use super::steps_sd::first_line_300;

const REACH_SECS: u64 = 20;
const UNKNOWN: &str = "this Mac doesn't know the primary's address yet";

pub fn is_tailscale_ip(ip: &str) -> bool {
    let Ok(std::net::IpAddr::V4(v4)) = ip.parse::<std::net::IpAddr>() else {
        return false;
    };
    let o = v4.octets();
    o[0] == 100 && (o[1] & 0xC0) == 0x40
}

pub(super) fn reach(m: &Machine) -> SetupStep {
    SetupStep::new(
        StepId::NODE_REACH,
        "Reaches the Primary",
        vec![StepId::NODE_SERVICE, StepId::NODE_TAILSCALE],
        Cadence::EveryRefresh,
        Probe::local(check),
        StepFix::HowTo(format!(
            "Allow tcp:47823 between your machines in the tailnet's access controls, so {} can reach the primary.",
            m.name
        )),
    )
}

fn check(ctx: &StepContext) -> CheckResult {
    let Some((addr, port)) = ctx.tokens.primary_addr() else {
        return CheckResult::cant_tell(UNKNOWN);
    };
    if !is_tailscale_ip(&addr) || port == 0 {
        return CheckResult::cant_tell(UNKNOWN);
    }
    if let Some(why) = machine_refusal(&ctx.machine) {
        return CheckResult::cant_tell(why);
    }
    let script = format!("{} cluster reach {addr} {port}", shell::path(BINARY));
    let ran = ctx.runner.run(&Call::secs(
        shell::ssh_script(&ctx.machine, &script),
        REACH_SECS,
    ));
    match ran.code {
        0 => CheckResult::pass(format!("reaches {addr}:{port}")),
        1 => {
            let said = if ran.err.trim().is_empty() {
                &ran.out
            } else {
                &ran.err
            };
            let line = first_line_300(&first_text_line(said), &|s| ctx.mask(s));
            CheckResult::fail(if line.is_empty() {
                format!("no answer from {addr}:{port}")
            } else {
                line
            })
        }
        code => CheckResult::cant_tell(format!("couldn't ask the machine (exit {code})")),
    }
}
