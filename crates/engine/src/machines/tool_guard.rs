use super::registry::Machine;
use super::runner::{Call, Runner, CANCELLED};
use super::shell;
use super::steps_node::BINARY;
use crate::versions::worker_update::{busy_script, busy_text, parse_busy};
use crate::versions::Tool;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const NODE_SECS: u64 = 30;
const BUSY_SECS: u64 = 30;

pub(super) type Cancel = Option<Arc<AtomicBool>>;

fn stopped(cancel: &Cancel) -> bool {
    cancel.as_ref().is_some_and(|c| c.load(Ordering::SeqCst))
}

fn call(machine: &Machine, script: &str, secs: u64, cancel: &Cancel) -> Call {
    let call = Call::secs(shell::ssh_script(machine, script), secs);
    match cancel {
        Some(flag) => call.cancellable(flag.clone()),
        None => call,
    }
}

pub(super) fn node_of(runner: &dyn Runner, machine: &Machine, cancel: &Cancel) -> Option<String> {
    if stopped(cancel) {
        return None;
    }
    let script = format!("{} cluster status --json", shell::path(BINARY));
    let result = runner.run(&call(machine, &script, NODE_SECS, cancel));
    if result.code != 0 {
        return None;
    }
    let value: Value = serde_json::from_str(result.out.trim()).ok()?;
    value["me"]
        .as_str()
        .filter(|me| !me.is_empty())
        .map(str::to_owned)
}

pub(super) fn verify_node(
    runner: &dyn Runner,
    machine: &Machine,
    node: &str,
    cancel: &Cancel,
) -> Result<(), String> {
    let name = &machine.name;
    match node_of(runner, machine, cancel) {
        Some(found) if found == node => Ok(()),
        Some(_) => Err(format!(
            "{name} is not the machine on this card: the Notchling there is another node. Nothing was changed."
        )),
        None => Err(format!(
            "Couldn't confirm which machine {name} is. Nothing was changed."
        )),
    }
}

pub(super) fn guard(
    runner: &dyn Runner,
    machine: &Machine,
    tool: Tool,
    node: &str,
    cancel: &Cancel,
) -> Result<(), String> {
    verify_node(runner, machine, node, cancel)?;
    let name = &machine.name;
    if stopped(cancel) {
        return Err(format!("Cancelled. Nothing was changed on {name}."));
    }
    let probe = runner.run(&call(machine, &busy_script(tool), BUSY_SECS, cancel));
    if probe.code == CANCELLED {
        return Err(format!("Cancelled. Nothing was changed on {name}."));
    }
    let count = if probe.code == 0 {
        parse_busy(&probe.out)
    } else {
        None
    };
    match count {
        Some(0) => Ok(()),
        Some(count) => Err(busy_text(name, tool, count)),
        None => Err(format!(
            "Couldn't tell whether {} is running on {name}. Nothing was changed.",
            tool.title()
        )),
    }
}
