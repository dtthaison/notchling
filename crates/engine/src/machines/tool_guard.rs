use super::runner::Call;
use super::shell;
use super::step::FixContext;
use super::steps_node::{json_from, BINARY};
use crate::versions::worker_update::{busy_script, busy_text, parse_busy};
use crate::versions::Tool;

const NODE_SECS: u64 = 30;
const BUSY_SECS: u64 = 30;

pub(super) fn node_of(ctx: &FixContext) -> Option<String> {
    let script = format!("{} cluster status --json", shell::path(BINARY));
    let call = Call::secs(shell::ssh_script(&ctx.step.machine, &script), NODE_SECS);
    let result = ctx.step.runner.run(&call);
    json_from(&result.out)?["me"].as_str().map(str::to_owned)
}

pub(super) fn guard(ctx: &FixContext, tool: Tool, node: &str) -> Result<(), String> {
    let name = &ctx.step.machine.name;
    match node_of(ctx) {
        Some(found) if found == node => {}
        Some(_) => {
            return Err(format!(
                "{name} is not the machine on this card: the Notchling there is another node. Nothing was changed."
            ))
        }
        None => {
            return Err(format!(
                "Couldn't confirm which machine {name} is. Nothing was changed."
            ))
        }
    }
    let call = Call::secs(
        shell::ssh_script(&ctx.step.machine, &busy_script(tool)),
        BUSY_SECS,
    );
    let probe = ctx.step.runner.run(&call);
    match parse_busy(&probe.out) {
        Some(0) => Ok(()),
        Some(count) => Err(busy_text(name, tool, count)),
        None => Err(format!(
            "Couldn't tell whether {} is running on {name}. Nothing was changed.",
            tool.title()
        )),
    }
}
