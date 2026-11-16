use super::registry::Machine;
use super::step::{
    Cadence, CheckResult, Probe, RemoteOutput, SetupStep, StepContext, StepFix, StepId,
};
use crate::cluster::logins::{agy_state, codex_state, how_to, AGY_LOGIN_FILE};
use crate::versions::Tool;
use std::path::Path;

const NONE: &str = "@@none";
const PRESENT: &str = "@@present";
const MISSING: &str = "@@missing";
const TIMEOUT: &str = "@@timeout";
const EXIT: &str = "@@exit=";

pub(super) const WAIT_TENTHS: u32 = 50;

pub(super) fn codex_script(tenths: u32) -> String {
    format!(
        "command -v codex >/dev/null 2>&1 || {{ echo @@none; exit 0; }}; o=$(mktemp) || exit 0; codex login status >\"$o\" 2>&1 </dev/null & p=$!; i=0; while kill -0 $p 2>/dev/null && [ $i -lt {tenths} ]; do sleep 0.1; i=$((i+1)); done; if kill -0 $p 2>/dev/null; then kill $p 2>/dev/null; echo @@timeout; else wait $p; c=$?; head -c 400 \"$o\"; echo; echo \"@@exit=$c\"; fi; rm -f \"$o\""
    )
}

pub(super) const AGY_SCRIPT: &str = "command -v agy >/dev/null 2>&1 || { echo @@none; exit 0; }; if [ -s \"$HOME/.gemini/jetski-standalone-oauth-token\" ]; then echo @@present; else echo @@missing; fi";

pub(super) fn steps(m: &Machine) -> Vec<SetupStep> {
    let mac = !m.is_linux();
    vec![
        SetupStep::new(
            StepId::NODE_CODEX_LOGIN,
            "Codex signed in",
            vec![StepId::REACHABLE],
            Cadence::EveryRefresh,
            Probe::remote(codex_script(WAIT_TENTHS), parse_codex),
            StepFix::HowTo(how_to(Tool::Codex, &m.name)),
        ),
        SetupStep::new(
            StepId::NODE_AGY_LOGIN,
            "Antigravity signed in",
            vec![StepId::REACHABLE],
            Cadence::EveryRefresh,
            Probe::remote(AGY_SCRIPT, move |out, ctx| parse_agy(out, ctx, mac)),
            StepFix::HowTo(how_to(Tool::Agy, &m.name)),
        ),
    ]
}

fn exit_code(text: &str) -> Option<i32> {
    text.lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix(EXIT))
        .and_then(|n| n.trim().parse().ok())
}

fn parse_codex(out: &RemoteOutput, _ctx: &StepContext) -> CheckResult {
    if out.text.lines().any(|l| l.trim() == NONE) {
        return CheckResult::cant_tell("Codex isn't installed there");
    }
    if out.text.lines().any(|l| l.trim() == TIMEOUT) {
        return CheckResult::cant_tell("Codex didn't answer in time there");
    }
    let Some(exit) = exit_code(&out.text) else {
        return CheckResult::cant_tell("couldn't ask Codex there");
    };
    match codex_state(exit, &out.text) {
        Some(true) => CheckResult::pass("signed in"),
        Some(false) => CheckResult::fail("not signed in"),
        None => CheckResult::cant_tell("couldn't tell from what Codex said"),
    }
}

fn parse_agy(out: &RemoteOutput, _ctx: &StepContext, mac: bool) -> CheckResult {
    let seen = |word: &str| out.text.lines().any(|l| l.trim() == word);
    if seen(NONE) {
        return CheckResult::cant_tell("Antigravity CLI isn't installed there");
    }
    let present = seen(PRESENT);
    if !present && !seen(MISSING) {
        return CheckResult::cant_tell("couldn't look for its login file there");
    }
    let file = |_: &Path| present;
    let os = if mac { "macos" } else { "linux" };
    match agy_state(os, &file, Path::new("/")) {
        Some(true) => CheckResult::pass("signed in"),
        Some(false) => CheckResult::fail(format!("not signed in (no {AGY_LOGIN_FILE})")),
        None => CheckResult::cant_tell(
            "no login file there; on a Mac Antigravity may keep its login in the Keychain, which Notchling doesn't read",
        ),
    }
}
