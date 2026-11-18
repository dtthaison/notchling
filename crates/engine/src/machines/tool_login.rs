use super::device_login::{DeviceFilter, PromptSink};
use super::engine::SetupEngine;
use super::runner::{Call, Channel, OutputSink, CANCELLED, TIMED_OUT};
use super::shell;
use super::step::{CheckState, FixContext, FixOutcome, StepId};
use super::steps_node_logins::{agy_check, codex_check, codex_script, AGY_SCRIPT, WAIT_TENTHS};
use crate::cluster::logins::{how_to, CODEX_COMMAND};
use crate::versions::Tool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const LOGIN_SECS: u64 = 600;
const WATCHDOG_SECS: u64 = LOGIN_SECS + 10;
const CHECK_SECS: u64 = 60;
const WATCHDOG_KILLED: i32 = 143;

pub fn can_sign_in(tool: Tool) -> bool {
    tool == Tool::Codex
}

pub fn login_script() -> String {
    format!(
        "command -v codex >/dev/null 2>&1 || {{ echo @@none; exit 127; }}; {CODEX_COMMAND} </dev/null & p=$!; ( sleep {WATCHDOG_SECS}; kill $p 2>/dev/null ) >/dev/null 2>&1 & w=$!; wait $p; c=$?; kill $w 2>/dev/null; exit $c"
    )
}

pub fn how_to_text(tool: Tool, machine: &str) -> String {
    let base = how_to(tool, machine);
    match tool {
        Tool::Agy => format!(
            "Antigravity has no sign-in Notchling can start on {machine}. {base} Then choose Check Again."
        ),
        _ => base,
    }
}

fn tail_of(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .collect();
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}

impl SetupEngine {
    pub fn sign_in_tool(
        &self,
        machine: &str,
        tool: Tool,
        prompt: PromptSink,
        cancel: Arc<AtomicBool>,
    ) -> FixOutcome {
        if !can_sign_in(tool) {
            return FixOutcome::fail(how_to_text(tool, machine));
        }
        let Some(m) = self.machine(machine) else {
            return FixOutcome::fail(format!("no machine named {machine}"));
        };
        let title = format!("Sign in {} on {machine}", tool.title());
        let outcome = self.perform(StepId::tool_login(tool), &title, &m, None, |ctx| {
            self.login_one(ctx, tool, prompt, &cancel)
        });
        self.refresh(Some(&[machine.to_string()]), false);
        outcome
    }

    pub fn check_login(&self, machine: &str, tool: Tool) -> FixOutcome {
        let Some(m) = self.machine(machine) else {
            return FixOutcome::fail(format!("no machine named {machine}"));
        };
        let outcome = self.recheck(&m, tool, &self.masks(&m, None));
        self.refresh(Some(&[machine.to_string()]), false);
        outcome
    }

    fn recheck(
        &self,
        m: &super::registry::Machine,
        tool: Tool,
        masks: &super::masks::Masks,
    ) -> FixOutcome {
        let name = &m.name;
        let mac = !m.is_linux();
        let script = match tool {
            Tool::Codex => codex_script(WAIT_TENTHS),
            Tool::Agy => AGY_SCRIPT.to_string(),
            _ => return FixOutcome::fail(format!("{} has no sign-in to check", tool.title())),
        };
        let call = Call::secs(shell::ssh_script(m, &script), CHECK_SECS);
        let result = self.deps.runner.run(&call);
        let check = match tool {
            Tool::Codex => codex_check(&result.out),
            _ => agy_check(&result.out, mac),
        };
        let text = masks.apply(check.state.text());
        match check.state {
            CheckState::Pass(_) => {
                FixOutcome::ok(format!("{} is signed in on {name}", tool.title()))
            }
            CheckState::Fail(_) => {
                FixOutcome::fail(format!("{} isn't signed in on {name}", tool.title()))
            }
            CheckState::CantTell(_) => FixOutcome::fail(format!(
                "Couldn't tell whether {} is signed in on {name}: {text}",
                tool.title()
            )),
        }
    }

    fn login_one(
        &self,
        ctx: &FixContext,
        tool: Tool,
        prompt: PromptSink,
        cancel: &Arc<AtomicBool>,
    ) -> FixOutcome {
        let m = &ctx.step.machine;
        let name = &m.name;
        let filter = Arc::new(DeviceFilter::new(ctx.log.clone(), prompt));
        let feed = filter.clone();
        let sink: Arc<OutputSink> = Arc::new(move |c: Channel, t: &str| feed.feed(c, t));
        (ctx.log)(Channel::Stdout, &format!("$ {CODEX_COMMAND}\n"));
        let mut argv = shell::ssh_script(m, &login_script());
        argv.splice(
            1..1,
            ["-o".to_string(), "ServerAliveInterval=30".to_string()],
        );
        let call = Call::secs(argv, LOGIN_SECS)
            .streaming(Some(sink))
            .cancellable(cancel.clone());
        let result = ctx.step.runner.run(&call);
        filter.finish();
        if cancel.load(Ordering::SeqCst) || result.code == CANCELLED {
            return FixOutcome::fail(format!(
                "Signing in {} on {name} was cancelled",
                tool.title()
            ));
        }
        if result.code == TIMED_OUT || result.code == WATCHDOG_KILLED {
            return FixOutcome::fail(format!(
                "Nobody approved the sign-in for {} on {name} within {} minutes",
                tool.title(),
                LOGIN_SECS / 60
            ));
        }
        if result.code != 0 {
            let said = if result.err.trim().is_empty() {
                &result.out
            } else {
                &result.err
            };
            let shown = ctx.mask(&tail_of(&filter.scrub(said)));
            let mut line = format!("{} didn't sign in on {name}", tool.title());
            if !shown.is_empty() {
                line.push_str(": ");
                line.push_str(&shown);
            }
            return FixOutcome::fail(line);
        }
        self.recheck(m, tool, &ctx.masks)
    }
}
