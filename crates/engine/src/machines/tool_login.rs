use super::device_login::{DeviceFilter, PromptSink};
use super::engine::SetupEngine;
use super::masks::Masks;
use super::registry::Machine;
use super::runner::{Call, Channel, OutputSink, CANCELLED, TIMED_OUT};
use super::shell;
use super::step::{CheckResult, CheckState, FixContext, FixOutcome, StepId};
use super::steps_node_logins::{agy_check, agy_script, codex_check, codex_script, WAIT_TENTHS};
use super::tool_guard::guard;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginCheck {
    pub outcome: FixOutcome,
    pub signed: Option<bool>,
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

fn step_of(tool: Tool) -> Option<StepId> {
    match tool {
        Tool::Codex => Some(StepId::NODE_CODEX_LOGIN),
        Tool::Agy => Some(StepId::NODE_AGY_LOGIN),
        _ => None,
    }
}

impl SetupEngine {
    pub fn sign_in_tool(
        &self,
        machine: &str,
        tool: Tool,
        node: &str,
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
            self.login_one(ctx, tool, node, prompt, &cancel)
        });
        self.changed();
        outcome
    }

    pub fn check_login(&self, machine: &str, tool: Tool) -> LoginCheck {
        let Some(m) = self.machine(machine) else {
            return LoginCheck {
                outcome: FixOutcome::fail(format!("no machine named {machine}")),
                signed: None,
            };
        };
        self.recheck(&m, tool, &self.masks(&m, None))
    }

    fn recheck(&self, m: &Machine, tool: Tool, masks: &Masks) -> LoginCheck {
        let name = &m.name;
        let mac = !m.is_linux();
        let script = match tool {
            Tool::Codex => codex_script(WAIT_TENTHS),
            Tool::Agy => agy_script(mac),
            _ => {
                return LoginCheck {
                    outcome: FixOutcome::fail(format!("{} has no sign-in to check", tool.title())),
                    signed: None,
                }
            }
        };
        let call = Call::secs(shell::ssh_script(m, &script), CHECK_SECS);
        let result = self.deps.runner.run(&call);
        let check: CheckResult = match tool {
            Tool::Codex => codex_check(&result.out),
            _ => agy_check(&result.out, mac),
        };
        let text = masks.apply(check.state.text());
        let (outcome, signed) = match &check.state {
            CheckState::Pass(_) => (
                FixOutcome::ok(format!("{} is signed in on {name}", tool.title())),
                Some(true),
            ),
            CheckState::Fail(_) => (
                FixOutcome::fail(format!("{} isn't signed in on {name}", tool.title())),
                Some(false),
            ),
            CheckState::CantTell(_) => (
                FixOutcome::fail(format!(
                    "Couldn't tell whether {} is signed in on {name}: {text}",
                    tool.title()
                )),
                None,
            ),
        };
        if let Some(id) = step_of(tool) {
            self.show(name, id, check);
        }
        LoginCheck { outcome, signed }
    }

    fn login_one(
        &self,
        ctx: &FixContext,
        tool: Tool,
        node: &str,
        prompt: PromptSink,
        cancel: &Arc<AtomicBool>,
    ) -> FixOutcome {
        let m = &ctx.step.machine;
        let name = &m.name;
        if let Err(why) = guard(ctx, tool, node) {
            return FixOutcome::fail(why);
        }
        match self.recheck(m, tool, &ctx.masks).signed {
            Some(false) => {}
            Some(true) => {
                return FixOutcome::fail(format!(
                    "{} is already signed in on {name}. Nothing was changed.",
                    tool.title()
                ))
            }
            None => {
                return FixOutcome::fail(format!(
                    "Couldn't tell whether {} is signed in on {name}. Nothing was changed.",
                    tool.title()
                ))
            }
        }
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
        let code = result.code;
        let said = if result.err.trim().is_empty() {
            result.out.clone()
        } else {
            result.err.clone()
        };
        drop(result);
        let hidden = filter.triggered();
        let shown = if hidden {
            String::new()
        } else {
            ctx.mask(&tail_of(&filter.scrub(&said)))
        };
        drop(said);
        if cancel.load(Ordering::SeqCst) || code == CANCELLED {
            return FixOutcome::fail(format!(
                "Signing in {} on {name} was cancelled",
                tool.title()
            ));
        }
        if code == TIMED_OUT || code == WATCHDOG_KILLED {
            return FixOutcome::fail(format!(
                "Nobody approved the sign-in for {} on {name} within {} minutes",
                tool.title(),
                LOGIN_SECS / 60
            ));
        }
        if code != 0 {
            let mut line = format!("{} didn't sign in on {name}", tool.title());
            if hidden {
                line.push_str(&format!(" (exit {code})"));
            } else if !shown.is_empty() {
                line.push_str(": ");
                line.push_str(&shown);
            }
            return FixOutcome::fail(line);
        }
        self.recheck(m, tool, &ctx.masks).outcome
    }
}
