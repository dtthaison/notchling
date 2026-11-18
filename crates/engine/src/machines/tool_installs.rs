use super::engine::SetupEngine;
use super::engine_plan::{AdminNeed, ApprovedStep, PlanId, PlanKind, ShownPlan};
use super::runner::Channel;
use super::step::{run, FixContext, FixOutcome, StepId};
use super::tool_updates::read_tools;
use crate::versions::worker_install::{install_plan, verdict};
use crate::versions::worker_update::failure_text;
use crate::versions::{Tool, ToolState, Version};

const INSTALL_SECS: u64 = 900;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolInstall {
    pub tool: Tool,
    pub mac: Option<Version>,
}

impl SetupEngine {
    pub(super) fn install_tool_plan(
        &self,
        machine: &str,
        want: &ToolInstall,
    ) -> Result<ShownPlan, String> {
        self.machine(machine)
            .ok_or(format!("no machine named {machine}"))?;
        let tools = self
            .worker_tools(machine)
            .ok_or(format!("{machine}'s tools aren't read yet"))?;
        let state = tools
            .tools
            .get(&want.tool)
            .unwrap_or(&ToolState::NotInstalled);
        let plan = install_plan(machine, want.tool, state, want.mac.as_ref())?;
        let title = format!("Install {} on {machine}", want.tool.title());
        Ok(ShownPlan {
            id: PlanId(0),
            machine: machine.to_string(),
            kind: PlanKind::InstallTool(want.clone()),
            title: title.clone(),
            steps: vec![ApprovedStep {
                id: StepId::tool_install(want.tool),
                title,
                display: plan.script,
                waiting: false,
                needs_admin: false,
            }],
            admin: AdminNeed::None,
        })
    }

    pub(super) fn install_tool(
        &self,
        machine: &str,
        want: &ToolInstall,
        steps: &[ApprovedStep],
    ) -> FixOutcome {
        let Some(step) = steps.first() else {
            return FixOutcome::fail("the install needs a look; check the machine again");
        };
        let Some(m) = self.machine(machine) else {
            return FixOutcome::fail(format!("no machine named {machine}"));
        };
        let target = want.mac.clone().filter(|_| want.tool == Tool::Codex);
        let outcome = self.perform(
            StepId::tool_install(want.tool),
            &step.title,
            &m,
            None,
            |ctx| self.install_one(ctx, want.tool, target.as_ref(), &step.display),
        );
        self.refresh(Some(&[machine.to_string()]), false);
        outcome
    }

    fn install_one(
        &self,
        ctx: &FixContext,
        tool: Tool,
        target: Option<&Version>,
        script: &str,
    ) -> FixOutcome {
        let m = &ctx.step.machine;
        let name = &m.name;
        (ctx.log)(Channel::Stdout, &format!("$ {script}\n"));
        let result = run::script(ctx, script, None, INSTALL_SECS);
        if result.code != 0 {
            return FixOutcome::fail(ctx.mask(&failure_text(tool, name, &result.out, &result.err)));
        }
        let tools = match read_tools(ctx.step.runner.as_ref(), m, &ctx.masks, (self.deps.clock)()) {
            Ok(tools) => tools,
            Err(why) => {
                return FixOutcome::fail(format!(
                    "{} was installed on {name}, but it couldn't be found afterwards: {why}",
                    tool.title()
                ))
            }
        };
        self.remember_tools(name, &tools);
        match verdict(name, tool, target, tools.tools.get(&tool)) {
            Ok(text) => FixOutcome::ok(text),
            Err(text) => FixOutcome::fail(text),
        }
    }
}
