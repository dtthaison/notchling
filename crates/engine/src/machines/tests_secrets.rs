//! Secrets: usages, delivery, verification, the two steps, token creation, renames (Swift
//! `MachinesSecretsTests`). No ssh starts and no vault is read; every value is a fake.

use super::fakes::*;
use super::registry::Machine;
use super::runner::{Channel, CommandResult, ScriptedRunner};
use super::shell;
use super::step::{CheckState, Probe, RemoteOutput};
use super::steps_secret::{deliver, usages, verify_probe};
use super::usage::{SecretFormat, SecretUsage};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub(super) const CLAUDE_VALUE: &str = "sk-ant-oat01-SECRETVALUE";

pub(super) fn claude_usage(machine: &str) -> SecretUsage {
    SecretUsage {
        name: "claude-worker".into(),
        machine: machine.into(),
        remote_path: shell::CLAUDE_ENV.into(),
        format: SecretFormat::KeyValue(shell::CLAUDE_KEY.into()),
    }
}

pub(super) fn raw_usage() -> SecretUsage {
    SecretUsage {
        name: "m1-github-issues".into(),
        machine: "m1".into(),
        remote_path: shell::GITHUB_TOKEN.into(),
        format: SecretFormat::Raw,
    }
}

pub(super) fn mac() -> Machine {
    TestMachines::mac("m1")
}

pub(super) fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(super) fn deliver_with(
    usage: &SecretUsage,
    runner: &Arc<ScriptedRunner>,
    values: &[(&str, &str)],
) -> (super::step::FixOutcome, Arc<LogBox>) {
    let ctx = Ctx::mac().runner(runner).secrets(values);
    let (fix, log) = ctx.fix(None);
    (deliver(usage, &fix), log)
}

// MARK: Usages

#[test]
fn usages_of_every_machine() {
    let list = usages(&[mac(), TestMachines::linux("vps-sing-1")]);
    assert_eq!(list.len(), 4);
    let claude: Vec<&SecretUsage> = list.iter().filter(|u| u.name == "claude-worker").collect();
    assert_eq!(
        claude
            .iter()
            .map(|u| u.machine.as_str())
            .collect::<Vec<_>>(),
        vec!["m1", "vps-sing-1"]
    );
    assert!(claude
        .iter()
        .all(|u| u.remote_path == "~/.config/agents/claude.env"
            && u.format == SecretFormat::KeyValue("CLAUDE_CODE_OAUTH_TOKEN".into())));
    assert!(list.contains(&raw_usage()));
    assert!(list.contains(&SecretUsage {
        name: "vps-sing-1-github-issues".into(),
        machine: "vps-sing-1".into(),
        remote_path: "~/.config/agents/github-token".into(),
        format: SecretFormat::Raw,
    }));
}

// MARK: Delivery

#[test]
fn delivery_puts_the_value_only_on_stdin() {
    let runner = Arc::new(ScriptedRunner::new());
    let (outcome, _) = deliver_with(
        &claude_usage("m1"),
        &runner,
        &[("claude-worker", CLAUDE_VALUE)],
    );
    assert!(outcome.ok, "{}", outcome.message);
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].stdin_text(),
        format!("CLAUDE_CODE_OAUTH_TOKEN={CLAUDE_VALUE}\n")
    );
    assert!(!calls[0].argv.iter().any(|a| a.contains("SECRETVALUE")));
    assert!(!calls[0].env.iter().any(|(_, v)| v.contains("SECRETVALUE")));
    assert_eq!(
        calls[0].argv,
        shell::ssh_script(
            &mac(),
            &shell::write_file("~/.config/agents/claude.env", "600")
        )
    );
    assert!(!outcome.message.contains("SECRETVALUE"));
}

/// A value with quotes, `$(...)` and backticks is still only bytes on stdin: no argv holds a piece
/// of it.
#[test]
fn a_hostile_looking_value_never_reaches_a_command_line() {
    let value = "tok'; $(id) `x` \"q\" 1234";
    let runner = Arc::new(ScriptedRunner::new());
    let (outcome, _) = deliver_with(&raw_usage(), &runner, &[("m1-github-issues", value)]);
    assert!(outcome.ok, "{}", outcome.message);
    let call = &runner.calls()[0];
    assert_eq!(call.stdin_text(), value);
    assert!(!call.argv.join(" ").contains("$(id)"));
    assert!(call.env.is_empty());
}

#[test]
fn a_machine_that_echoes_the_value_never_shows_it_in_the_outcome_or_the_log() {
    let echo = format!("cat: {CLAUDE_VALUE} CLAUDE_CODE_OAUTH_TOKEN={CLAUDE_VALUE}");
    let runner = Arc::new(ScriptedRunner::new());
    runner.on_with(
        |_| true,
        CommandResult::new(1, "", &echo),
        vec![
            (Channel::Stderr, echo.clone()),
            (Channel::Stdout, format!("again {CLAUDE_VALUE}\n")),
        ],
    );
    let (outcome, log) = deliver_with(
        &claude_usage("m1"),
        &runner,
        &[("claude-worker", CLAUDE_VALUE)],
    );
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("[secret:claude-worker]"),
        "{}",
        outcome.message
    );
    assert!(
        !outcome.message.contains("SECRETVALUE"),
        "{}",
        outcome.message
    );
    assert!(!log.joined().contains("SECRETVALUE"), "{}", log.joined());
    assert!(log.joined().contains("[secret:claude-worker]"));
}

#[test]
fn a_too_short_value_is_refused() {
    let runner = Arc::new(ScriptedRunner::new());
    let (outcome, _) = deliver_with(
        &claude_usage("m1"),
        &runner,
        &[("claude-worker", "abc1234")],
    );
    assert!(!outcome.ok);
    assert_eq!(
        outcome.message,
        "the value of claude-worker is too short; save it again"
    );
    assert!(runner.calls().is_empty());
}

/// A realistic Claude token is 108 characters. The echo starts 250 characters in, so a cut at 300
/// would split it; no 8-character piece of it may reach the message.
#[test]
fn an_echo_that_starts_late_is_redacted_before_the_message_is_cut() {
    let alphabet: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
        .chars()
        .collect();
    let body: String = (0..95)
        .map(|i| alphabet[(i * 7) % alphabet.len()])
        .collect();
    let value = format!("sk-ant-oat01-{body}");
    assert_eq!(value.chars().count(), 108);
    for on_stderr in [true, false] {
        let said = format!("{}{value} tail", "x".repeat(250));
        let result = if on_stderr {
            CommandResult::new(1, "", &said)
        } else {
            CommandResult::new(1, &said, "")
        };
        let runner = Arc::new(ScriptedRunner::new());
        runner.on(|_| true, result);
        let (outcome, _) = deliver_with(&claude_usage("m1"), &runner, &[("claude-worker", &value)]);
        assert!(!outcome.ok);
        assert_no_fragment(&value, &outcome.message);
        assert!(
            outcome.message.contains("[secret:claude-worker]"),
            "{}",
            outcome.message
        );
    }
}

/// Mask first, trim afterwards: a value with a space at its edge no longer matches once the edge
/// is trimmed.
#[test]
fn a_value_with_edge_spaces_echoed_alone_is_still_masked() {
    let value = "  sk-ant-oat01-SPACEDVALUE  ";
    for on_stderr in [true, false] {
        let result = if on_stderr {
            CommandResult::new(1, "", value)
        } else {
            CommandResult::new(1, value, "")
        };
        let runner = Arc::new(ScriptedRunner::new());
        runner.on(|_| true, result);
        let (outcome, _) = deliver_with(&claude_usage("m1"), &runner, &[("claude-worker", value)]);
        assert!(!outcome.ok);
        assert!(
            !outcome.message.contains("SPACEDVALUE"),
            "{}",
            outcome.message
        );
        assert!(
            outcome.message.contains("[secret:claude-worker]"),
            "{}",
            outcome.message
        );
    }
}

/// The engine's mask (every vault value of the machine) is applied to deliver's message too.
#[test]
fn deliver_runs_its_message_through_the_contexts_mask() {
    let runner = Arc::new(ScriptedRunner::new());
    runner.on(
        |_| true,
        CommandResult::new(1, "", "no space for OTHERSECRET-1234 here"),
    );
    let ctx = Ctx::mac()
        .runner(&runner)
        .secrets(&[("claude-worker", CLAUDE_VALUE)])
        .hiding("other", "OTHERSECRET-1234");
    let (fix, _) = ctx.fix(None);
    let outcome = deliver(&claude_usage("m1"), &fix);
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("[secret:other]"),
        "{}",
        outcome.message
    );
    assert!(
        !outcome.message.contains("OTHERSECRET"),
        "{}",
        outcome.message
    );
}

#[test]
fn a_huge_or_cut_echo_is_not_shown_at_all() {
    for said in [
        format!("[earlier output cut]\nrest of {CLAUDE_VALUE}"),
        "y".repeat(5000),
    ] {
        let runner = Arc::new(ScriptedRunner::new());
        runner.on(|_| true, CommandResult::new(1, "", &said));
        let (outcome, _) = deliver_with(
            &claude_usage("m1"),
            &runner,
            &[("claude-worker", CLAUDE_VALUE)],
        );
        assert!(!outcome.ok);
        assert!(!outcome.message.contains("SECRETVALUE"));
        assert!(!outcome.message.contains("yyyy"));
        assert!(!outcome.message.contains("rest of"));
    }
}

#[test]
fn a_raw_token_has_no_newline() {
    let runner = Arc::new(ScriptedRunner::new());
    let (outcome, _) = deliver_with(
        &raw_usage(),
        &runner,
        &[("m1-github-issues", "github_pat_ABC")],
    );
    assert!(outcome.ok, "{}", outcome.message);
    let call = &runner.calls()[0];
    assert_eq!(call.stdin_text(), "github_pat_ABC");
    assert_eq!(
        call.argv,
        shell::ssh_script(&mac(), &shell::write_file(shell::GITHUB_TOKEN, "600"))
    );
}

#[test]
fn a_value_with_a_line_break_is_refused() {
    for bad in ["abc\ndef", "abc\rdef", "abc\0def"] {
        let runner = Arc::new(ScriptedRunner::new());
        let (outcome, _) = deliver_with(&claude_usage("m1"), &runner, &[("claude-worker", bad)]);
        assert!(!outcome.ok);
        assert_eq!(
            outcome.message,
            "the value of claude-worker has a line break; save it again"
        );
        assert!(runner.calls().is_empty());
    }
}

#[test]
fn a_missing_vault_entry_is_not_a_delivery() {
    let runner = Arc::new(ScriptedRunner::new());
    let (outcome, _) = deliver_with(&claude_usage("m1"), &runner, &[]);
    assert!(!outcome.ok);
    assert!(runner.calls().is_empty());
    let (empty, _) = deliver_with(&claude_usage("m1"), &runner, &[("claude-worker", "")]);
    assert!(!empty.ok);
    assert!(runner.calls().is_empty());
}

#[test]
fn a_usage_of_another_machine_is_not_delivered() {
    let runner = Arc::new(ScriptedRunner::new());
    let (outcome, _) = deliver_with(
        &claude_usage("vps-sing-1"),
        &runner,
        &[("claude-worker", CLAUDE_VALUE)],
    );
    assert!(!outcome.ok);
    assert!(runner.calls().is_empty());
}

#[test]
fn a_usage_whose_key_is_not_a_variable_name_is_refused() {
    let usage = SecretUsage {
        format: SecretFormat::KeyValue("A=B; rm -rf ~".into()),
        ..claude_usage("m1")
    };
    let runner = Arc::new(ScriptedRunner::new());
    let (outcome, _) = deliver_with(&usage, &runner, &[("claude-worker", CLAUDE_VALUE)]);
    assert!(!outcome.ok);
    assert!(outcome.message.contains("is not a variable name"));
    assert!(runner.calls().is_empty());
}

// MARK: Verification

pub(super) fn run_probe(
    probe: &Probe,
    text: &str,
    code: i32,
    values: &[(&str, &str)],
) -> CheckState {
    let Probe::Remote { parse, .. } = probe else {
        panic!("not a remote probe");
    };
    let ctx = Ctx::mac().secrets(values).step();
    parse(
        &RemoteOutput {
            text: text.into(),
            code,
        },
        &ctx,
    )
    .state
}

#[test]
fn verify_never_prints_the_files_content() {
    for linux in [false, true] {
        let probe = verify_probe(&claude_usage("m1"), linux);
        let script = probe.script().unwrap().to_string();
        // mode and size first, then the hash of the file; the content itself is never printed
        assert!(
            script.starts_with(&shell::stat(&[shell::CLAUDE_ENV], linux)),
            "{script}"
        );
        assert!(script.contains(if linux { "sha256sum" } else { "shasum -a 256" }));
        assert!(!script.contains("cat "));
        assert!(!script.contains("head"));
        assert!(!script.contains("grep"));
    }
    let probe = verify_probe(&claude_usage("m1"), false);
    assert_eq!(
        run_probe(&probe, "600 41\n", 0, &[]),
        CheckState::Pass("mode 600, 41 bytes".into())
    );
    assert_eq!(
        run_probe(&probe, "missing\n", 0, &[]),
        CheckState::Fail("not on the machine".into())
    );
    assert_eq!(
        run_probe(&probe, "644 41\n", 0, &[]),
        CheckState::Fail("mode 644, should be 600".into())
    );
    assert_eq!(
        run_probe(&probe, "600 0\n", 0, &[]),
        CheckState::Fail("empty".into())
    );
    assert!(matches!(
        run_probe(&probe, "Permission denied\n", 0, &[]),
        CheckState::CantTell(_)
    ));
    assert!(matches!(
        run_probe(&probe, "600 41\n", 255, &[]),
        CheckState::CantTell(_)
    ));
}

// MARK: Content check

fn text_of(state: &CheckState) -> &str {
    state.text()
}

/// The file on the machine is compared with the vault by hash, in memory. The result says "same" or
/// "different" and holds neither hash.
#[test]
fn verify_compares_the_files_hash_with_the_vaults_and_shows_neither_hash() {
    let values = [
        ("claude-worker", CLAUDE_VALUE),
        ("m1-github-issues", "github_pat_ABC"),
    ];
    let claude = verify_probe(&claude_usage("m1"), false);
    let same = sha(&format!("CLAUDE_CODE_OAUTH_TOKEN={CLAUDE_VALUE}\n"));
    let other = sha("CLAUDE_CODE_OAUTH_TOKEN=an-older-token\n");

    let ok = run_probe(&claude, &format!("600 41\n{same}\n"), 0, &values);
    assert_eq!(
        ok,
        CheckState::Pass("mode 600, 41 bytes, same as the vault".into())
    );
    let stale = run_probe(&claude, &format!("600 41\n{other}\n"), 0, &values);
    assert_eq!(
        stale,
        CheckState::Fail("different from the vault (Send again)".into())
    );
    for state in [&ok, &stale] {
        let text = text_of(state);
        assert!(
            !text.contains(&same) && !text.contains(&other),
            "no hash in a result"
        );
        assert!(!text.contains(CLAUDE_VALUE));
    }

    // the issues token is the raw value, no newline
    let issues = verify_probe(&raw_usage(), true);
    let raw_ok = run_probe(
        &issues,
        &format!("600 14\n{}\n", sha("github_pat_ABC")),
        0,
        &values,
    );
    assert_eq!(
        raw_ok,
        CheckState::Pass("mode 600, 14 bytes, same as the vault".into())
    );
    let raw_stale = run_probe(
        &issues,
        &format!("600 14\n{}\n", sha("github_pat_ABC\n")),
        0,
        &values,
    );
    assert_eq!(
        raw_stale,
        CheckState::Fail("different from the vault (Send again)".into()),
        "a trailing newline is a different file"
    );

    // upper case hex from the machine counts the same
    let upper = run_probe(
        &claude,
        &format!("600 41\n{}\n", same.to_uppercase()),
        0,
        &values,
    );
    assert_eq!(
        upper,
        CheckState::Pass("mode 600, 41 bytes, same as the vault".into())
    );
}

#[test]
fn the_content_check_comes_after_mode_and_size_and_needs_a_hash_to_say_anything() {
    let values = [("claude-worker", CLAUDE_VALUE)];
    let probe = verify_probe(&claude_usage("m1"), false);
    let hash = sha(&format!("CLAUDE_CODE_OAUTH_TOKEN={CLAUDE_VALUE}\n"));
    assert_eq!(
        run_probe(&probe, &format!("644 41\n{hash}\n"), 0, &values),
        CheckState::Fail("mode 644, should be 600".into())
    );
    assert_eq!(
        run_probe(&probe, "missing\n", 0, &values),
        CheckState::Fail("not on the machine".into())
    );
    // no hash line (the tool is missing) or a line that is not a hash: can't tell, with no part of
    // it shown
    let short = format!("600 41\n{}\n", &hash[..63]);
    let zs = format!("600 41\n{}\n", "z".repeat(64));
    for text in [
        "600 41\n",
        "600 41\nsha256sum: command not found\n",
        short.as_str(),
        zs.as_str(),
    ] {
        let state = run_probe(&probe, text, 0, &values);
        let CheckState::CantTell(why) = &state else {
            panic!("expected can't tell for {text}, got {state:?}");
        };
        assert!(!why.contains("sha256sum: command"), "{why}");
    }
    // nothing in the vault to compare with: mode and size only, as before
    assert_eq!(
        run_probe(&probe, &format!("600 41\n{hash}\n"), 0, &[]),
        CheckState::Pass("mode 600, 41 bytes".into())
    );
}
