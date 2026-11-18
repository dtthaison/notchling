use super::engine::{lock, SetupEngine};
use super::engine_plan::{AdminNeed, ApprovedStep, PlanId, PlanKind, ShownPlan};
use super::masks::Masks;
use super::registry::Machine;
use super::runner::{Call, Channel, Runner};
use super::shell;
use super::step::{run, FixContext, FixOutcome, StepId};
use super::steps_versions::{self, TOOL_VERSIONS_SCRIPT};
use crate::versions::worker_update::{
    busy_script, busy_text, failure_text, parse_busy, plan, Move, TOOLS,
};
use crate::versions::{Tool, ToolState, Version, WorkerTools};
use std::collections::BTreeSet;

const READ_SECS: u64 = 60;
const BUSY_SECS: u64 = 30;
const INSTALL_SECS: u64 = 900;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolMove {
    pub tool: Tool,
    pub to: Version,
}

pub fn read_tools(
    runner: &dyn Runner,
    machine: &Machine,
    masks: &Masks,
    now: f64,
) -> Result<WorkerTools, String> {
    let call = Call::secs(shell::ssh_script(machine, TOOL_VERSIONS_SCRIPT), READ_SECS);
    let result = runner.run(&call);
    steps_versions::read(&result.out, (now * 1000.0) as i64, masks).ok_or_else(|| {
        let why = result.err.trim();
        if why.is_empty() {
            format!("couldn't read the tools on {}", machine.name)
        } else {
            masks.apply(why)
        }
    })
}

impl SetupEngine {
    pub fn worker_tools(&self, machine: &str) -> Option<WorkerTools> {
        lock(&self.state).tools_known.get(machine).cloned()
    }

    pub fn read_worker_tools(&self, machine: &str) -> Result<WorkerTools, String> {
        let m = self
            .machine(machine)
            .ok_or(format!("no machine named {machine}"))?;
        let masks = self.masks(&m, None);
        let tools = read_tools(self.deps.runner.as_ref(), &m, &masks, (self.deps.clock)())?;
        self.remember_tools(machine, &tools);
        Ok(tools)
    }

    pub(super) fn remember_tools(&self, machine: &str, tools: &WorkerTools) {
        {
            lock(&self.state)
                .tools_known
                .insert(machine.to_string(), tools.clone());
        }
        let registry: Vec<String> = self
            .deps
            .store
            .machines()
            .into_iter()
            .map(|m| m.name)
            .collect();
        self.save_tools(vec![(machine.to_string(), tools.clone())], &registry);
        self.changed();
    }

    pub(super) fn update_tools_plan(
        &self,
        machine: &str,
        moves: &[ToolMove],
    ) -> Result<ShownPlan, String> {
        self.machine(machine)
            .ok_or(format!("no machine named {machine}"))?;
        if moves.is_empty() {
            return Err("no tool to update".into());
        }
        let mut seen = BTreeSet::new();
        if let Some(twice) = moves.iter().find(|mv| !seen.insert(mv.tool)) {
            return Err(format!("{} is in the update twice", twice.tool.title()));
        }
        let tools = self
            .worker_tools(machine)
            .ok_or(format!("{machine}'s tools aren't read yet"))?;
        let mut steps = Vec::new();
        for mv in moves {
            if !TOOLS.contains(&mv.tool) {
                return Err(format!("Notchling doesn't update {} here", mv.tool.title()));
            }
            let state = tools
                .tools
                .get(&mv.tool)
                .unwrap_or(&ToolState::NotInstalled);
            match plan(machine, mv.tool, state, &mv.to)? {
                Move::HowTo(text) => return Err(text),
                Move::Run(script) => steps.push(ApprovedStep {
                    id: StepId::tool_update(mv.tool),
                    title: format!("Update {} on {machine} to {}", mv.tool.title(), mv.to),
                    display: script,
                    waiting: false,
                    needs_admin: false,
                }),
            }
        }
        Ok(ShownPlan {
            id: PlanId(0),
            machine: machine.to_string(),
            kind: PlanKind::UpdateTools(moves.to_vec()),
            title: format!("Update tools on {machine}"),
            steps,
            admin: AdminNeed::None,
        })
    }

    pub(super) fn update_tools(
        &self,
        machine: &str,
        moves: &[ToolMove],
        steps: &[ApprovedStep],
    ) -> FixOutcome {
        if moves.len() != steps.len() {
            return FixOutcome::fail("the update needs a look; check the machine again");
        }
        let Some(m) = self.machine(machine) else {
            return FixOutcome::fail(format!("no machine named {machine}"));
        };
        let mut done: Vec<String> = Vec::new();
        for (mv, step) in moves.iter().zip(steps) {
            let outcome =
                self.perform(StepId::tool_update(mv.tool), &step.title, &m, None, |ctx| {
                    self.update_one(ctx, mv.tool, &mv.to, &step.display)
                });
            if !outcome.ok {
                self.refresh(Some(&[machine.to_string()]), false);
                let mut message = outcome.message;
                if !done.is_empty() {
                    message.push_str(&format!("\n{} already updated", done.join("; ")));
                }
                return FixOutcome::fail(message);
            }
            done.push(outcome.message);
        }
        self.refresh(Some(&[machine.to_string()]), false);
        FixOutcome::ok(done.join("\n"))
    }

    fn update_one(&self, ctx: &FixContext, tool: Tool, to: &Version, script: &str) -> FixOutcome {
        let m = &ctx.step.machine;
        let name = &m.name;
        let probe = run::script(ctx, &busy_script(tool), None, BUSY_SECS);
        match parse_busy(&probe.out) {
            Some(0) => {}
            Some(count) => return FixOutcome::fail(busy_text(name, tool, count)),
            None => {
                return FixOutcome::fail(format!(
                    "Couldn't tell whether {} is running on {name}. Nothing was changed.",
                    tool.title()
                ))
            }
        }
        (ctx.log)(Channel::Stdout, &format!("$ {script}\n"));
        let result = run::script(ctx, script, None, INSTALL_SECS);
        if result.code != 0 {
            return FixOutcome::fail(ctx.mask(&failure_text(tool, name, &result.out, &result.err)));
        }
        let tools = match read_tools(ctx.step.runner.as_ref(), m, &ctx.masks, (self.deps.clock)()) {
            Ok(tools) => tools,
            Err(why) => {
                return FixOutcome::fail(format!(
                    "{} was installed on {name}, but its version couldn't be read: {why}",
                    tool.title()
                ))
            }
        };
        self.remember_tools(name, &tools);
        match tools.tools.get(&tool) {
            Some(ToolState::Installed {
                version: Some(found),
                ..
            }) if found == to => FixOutcome::ok(format!("{} on {name} is now {to}", tool.title())),
            Some(ToolState::Installed {
                version: Some(found),
                ..
            }) => FixOutcome::fail(format!(
                "{} on {name} still reports {found}, not {to}",
                tool.title()
            )),
            _ => FixOutcome::fail(format!(
                "{} on {name} reports no version after the install; it should be {to}",
                tool.title()
            )),
        }
    }
}
