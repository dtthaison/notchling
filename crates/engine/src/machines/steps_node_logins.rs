use super::registry::Machine;
use super::step::{
    Cadence, CheckResult, Probe, RemoteOutput, SetupStep, StepContext, StepFix, StepId,
};
use crate::cluster::logins::{agy_state, how_to, keychain_command, AGY_LOGIN_FILE};
use crate::versions::Tool;
use std::path::Path;

const NONE: &str = "@@none";
const PRESENT: &str = "@@present";
const MISSING: &str = "@@missing";
const KEYCHAIN: &str = "@@keychain";
pub(super) const SIGNED_IN: &str = "signed_in";
pub(super) const SIGNED_OUT: &str = "signed_out";
pub(super) const API_KEY: &str = "api_key";
pub(super) const UNKNOWN: &str = "unknown";

pub(super) const WAIT_TENTHS: u32 = 50;

pub(super) fn codex_state_snippet(tenths: u32, none_exit: i32, fail_exit: i32) -> String {
    format!(
        "command -v codex >/dev/null 2>&1 || {{ echo @@none; exit {none_exit}; }}; o=$(mktemp) || {{ echo @@unknown; exit {fail_exit}; }}; codex login status >\"$o\" 2>&1 </dev/null & p=$!; i=0; while kill -0 $p 2>/dev/null && [ $i -lt {tenths} ]; do sleep 0.1; i=$((i+1)); done; if kill -0 $p 2>/dev/null; then kill $p 2>/dev/null; s=timeout; else wait $p; c=$?; s=unknown; if [ $c -eq 0 ]; then if grep -qi 'not logged in' \"$o\"; then s=unknown; elif grep -qi 'logged in' \"$o\"; then s=signed_in; if grep -qi 'api key' \"$o\"; then s=api_key; fi; fi; elif grep -qi 'not logged in' \"$o\"; then s=signed_out; fi; fi; rm -f \"$o\""
    )
}

pub(super) fn codex_script(tenths: u32) -> String {
    format!("{}; echo \"@@$s\"", codex_state_snippet(tenths, 0, 0))
}

pub(super) fn state_word(text: &str) -> Option<&str> {
    text.lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("@@"))
        .filter(|w| {
            matches!(
                *w,
                "none" | "timeout" | SIGNED_IN | SIGNED_OUT | API_KEY | UNKNOWN
            )
        })
}

pub(super) const AGY_SCRIPT: &str = "command -v agy >/dev/null 2>&1 || { echo @@none; exit 0; }; if [ -s \"$HOME/.gemini/jetski-standalone-oauth-token\" ]; then echo @@present; else echo @@missing; fi";

pub(super) fn agy_script(mac: bool) -> String {
    if !mac {
        return AGY_SCRIPT.to_string();
    }
    format!(
        "command -v agy >/dev/null 2>&1 || {{ echo @@none; exit 0; }}; if [ -s \"$HOME/.gemini/jetski-standalone-oauth-token\" ]; then echo @@present; else echo @@missing; if {} >/dev/null 2>&1; then echo {KEYCHAIN}; fi; fi",
        keychain_command()
    )
}

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
            Probe::remote(agy_script(mac), move |out, ctx| parse_agy(out, ctx, mac)),
            StepFix::HowTo(how_to(Tool::Agy, &m.name)),
        ),
    ]
}

fn parse_codex(out: &RemoteOutput, _ctx: &StepContext) -> CheckResult {
    codex_check(&out.text)
}

pub(super) fn codex_check(text: &str) -> CheckResult {
    match state_word(text) {
        Some("none") => CheckResult::cant_tell("Codex isn't installed there"),
        Some("timeout") => CheckResult::cant_tell("Codex didn't answer in time there"),
        Some(SIGNED_IN) => CheckResult::pass("signed in"),
        Some(SIGNED_OUT) => CheckResult::fail("not signed in"),
        Some(API_KEY) => CheckResult::cant_tell("signed in with an API key"),
        Some(_) => CheckResult::cant_tell("couldn't tell from what Codex said"),
        None => CheckResult::cant_tell("couldn't ask Codex there"),
    }
}

fn parse_agy(out: &RemoteOutput, _ctx: &StepContext, mac: bool) -> CheckResult {
    agy_check(&out.text, mac)
}

pub(super) fn agy_check(text: &str, mac: bool) -> CheckResult {
    let seen = |word: &str| text.lines().any(|l| l.trim() == word);
    if seen(NONE) {
        return CheckResult::cant_tell("Antigravity CLI isn't installed there");
    }
    let present = seen(PRESENT);
    if !present && !seen(MISSING) {
        return CheckResult::cant_tell("couldn't look for its login file there");
    }
    let file = |_: &Path| present;
    let os = if mac { "macos" } else { "linux" };
    if mac && !present && seen(KEYCHAIN) {
        return CheckResult::pass("signed in (Keychain)");
    }
    match agy_state(os, &file, Path::new("/")) {
        Some(true) => CheckResult::pass("signed in"),
        Some(false) => CheckResult::fail(format!("not signed in (no {AGY_LOGIN_FILE})")),
        None => CheckResult::cant_tell(
            "no login file or Keychain item found there; over ssh a Mac may not show its login Keychain",
        ),
    }
}
