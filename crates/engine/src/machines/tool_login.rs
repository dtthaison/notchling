use super::device_login::{DeviceFilter, PromptSink};
use super::engine::SetupEngine;
use super::masks::Masks;
use super::registry::Machine;
use super::runner::{Call, Channel, OutputSink, CANCELLED, TIMED_OUT};
use super::shell;
use super::step::{CheckResult, CheckState, FixContext, FixOutcome, StepId};
use super::steps_node_logins::{
    agy_check, agy_script, codex_check, codex_script, codex_state_snippet, state_word, API_KEY,
    SIGNED_IN, WAIT_TENTHS,
};
use super::tool_guard::{guard, verify_node};
use crate::cluster::logins::{how_to, CODEX_COMMAND};
use crate::versions::Tool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const LOGIN_SECS: u64 = 600;
const WATCHDOG_SECS: u64 = LOGIN_SECS + 10;
const CHECK_SECS: u64 = 60;
const WATCHDOG_KILLED: i32 = 143;
const REFUSED: i32 = 64;
const SSH_FAILED: i32 = 255;

pub fn can_sign_in(tool: Tool) -> bool {
    tool == Tool::Codex
}

pub fn login_script() -> String {
    format!(
        "{}; if [ \"$s\" != signed_out ]; then echo \"@@$s\"; exit {REFUSED}; fi; {CODEX_COMMAND} </dev/null & p=$!; ( sleep {WATCHDOG_SECS}; kill $p 2>/dev/null ) >/dev/null 2>&1 & w=$!; wait $p; c=$?; kill $w 2>/dev/null; exit $c",
        codex_state_snippet(WAIT_TENTHS, 127, REFUSED)
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

fn step_of(tool: Tool) -> Option<StepId> {
    match tool {
        Tool::Codex => Some(StepId::NODE_CODEX_LOGIN),
        Tool::Agy => Some(StepId::NODE_AGY_LOGIN),
        _ => None,
    }
}

fn cancelled_text(tool: Tool, name: &str) -> String {
    format!("Signing in {} on {name} was cancelled", tool.title())
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

    pub fn check_login(&self, machine: &str, tool: Tool, node: &str) -> LoginCheck {
        let Some(m) = self.machine(machine) else {
            return LoginCheck {
                outcome: FixOutcome::fail(format!("no machine named {machine}")),
                signed: None,
            };
        };
        if let Err(why) = verify_node(self.deps.runner.as_ref(), &m, node, &None) {
            return LoginCheck {
                outcome: FixOutcome::fail(why),
                signed: None,
            };
        }
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
        let stopped = || cancel.load(Ordering::SeqCst);
        if stopped() {
            return FixOutcome::fail(cancelled_text(tool, name));
        }
        let shared = Some(cancel.clone());
        if let Err(why) = guard(ctx.step.runner.as_ref(), m, tool, node, &shared) {
            return FixOutcome::fail(if stopped() {
                cancelled_text(tool, name)
            } else {
                why
            });
        }
        if stopped() {
            return FixOutcome::fail(cancelled_text(tool, name));
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
        let word = state_word(&result.out).map(str::to_owned);
        drop(result);
        if stopped() || code == CANCELLED {
            return FixOutcome::fail(cancelled_text(tool, name));
        }
        if code == REFUSED {
            return FixOutcome::fail(match word.as_deref() {
                Some(SIGNED_IN) | Some(API_KEY) => format!(
                    "{} is already signed in on {name}. Nothing was changed.",
                    tool.title()
                ),
                _ => format!(
                    "Couldn't tell whether {} is signed in on {name}. Nothing was changed.",
                    tool.title()
                ),
            });
        }
        if code == TIMED_OUT || code == WATCHDOG_KILLED {
            return FixOutcome::fail(format!(
                "Nobody approved the sign-in for {} on {name} within {} minutes",
                tool.title(),
                LOGIN_SECS / 60
            ));
        }
        if code == SSH_FAILED {
            return FixOutcome::fail(format!(
                "Couldn't reach {name} over ssh to sign {} in",
                tool.title()
            ));
        }
        if code == 127 || word.as_deref() == Some("none") {
            return FixOutcome::fail(format!("{} isn't installed on {name}", tool.title()));
        }
        if code != 0 {
            return FixOutcome::fail(format!(
                "{} didn't sign in on {name} (exit {code})",
                tool.title()
            ));
        }
        self.recheck(m, tool, &ctx.masks).outcome
    }
}
