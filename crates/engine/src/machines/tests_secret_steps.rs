//! The two secret steps, token creation, renames and the vault source (Swift
//! `MachinesSecretsTests`, second half).

use super::fakes::*;
use super::runner::ScriptedRunner;
use super::shell;
use super::step::{SecretSource, StepFix};
use super::steps_secret::{
    claude_login, issues_token, pending_renames, send_display, VaultSecretSource, LEGACY_NAMES,
};
use super::tests_secrets::{claude_usage, mac, raw_usage, CLAUDE_VALUE};
use super::token_creation;
use crate::vault::{expiry, Kind, VaultEntry, VaultValue};
use std::sync::Arc;

// MARK: Steps

#[test]
fn claude_login_fix_opens_the_form_when_the_vault_lacks_the_token() {
    let runner = Arc::new(ScriptedRunner::new());
    let ctx = Ctx::mac().runner(&runner);
    let step = claude_login(&ctx.machine);
    let StepFix::Run(plan) = &step.fix else {
        panic!("not a Fix");
    };
    let (fix, _) = ctx.fix(None);
    let outcome = (plan.run)(&fix);
    assert!(!outcome.ok);
    assert!(runner.calls().is_empty());
    let forms = ctx.opener.forms();
    assert_eq!(forms.len(), 1);
    let request = &forms[0];
    assert_eq!(request.name, "claude-worker");
    assert_eq!(request.kind.as_deref(), Some("token"));
    assert_eq!(request.url.as_deref(), Some("https://claude.ai"));
    assert_eq!(
        request.note.as_deref(),
        Some("Claude Code OAuth token of the separate worker account (every machine)")
    );
    assert_eq!(request.how_to.as_deref(), Some("In your own Terminal (not an agent): run claude setup-token, sign in with the worker account in a private browser window, paste the token here, then clear the Terminal."));
}

#[test]
fn claude_login_fix_delivers_when_the_vault_has_the_token() {
    let runner = Arc::new(ScriptedRunner::new());
    let ctx = Ctx::mac()
        .runner(&runner)
        .secrets(&[("claude-worker", CLAUDE_VALUE)]);
    let step = claude_login(&ctx.machine);
    let StepFix::Run(plan) = &step.fix else {
        panic!("not a Fix");
    };
    let (fix, _) = ctx.fix(None);
    let outcome = (plan.run)(&fix);
    assert!(outcome.ok, "{}", outcome.message);
    assert!(ctx.opener.forms().is_empty());
    assert_eq!(runner.calls().len(), 1);
    assert!(!plan.display.contains("SECRETVALUE"));
    assert!(plan.display.contains("[secret:claude-worker]"));
}

#[test]
fn issues_token_fix_opens_the_creation_flow_when_the_vault_lacks_it() {
    let machine = mac();
    let runner = Arc::new(ScriptedRunner::new());
    let ctx = Ctx::new(machine.clone()).runner(&runner);
    let step = issues_token(&machine, machine.sd_worker().unwrap());
    let StepFix::Run(plan) = &step.fix else {
        panic!("not a Fix");
    };
    let (fix, _) = ctx.fix(None);
    let outcome = (plan.run)(&fix);
    assert!(!outcome.ok);
    assert!(runner.calls().is_empty());
    let pages = ctx.opener.pages();
    assert_eq!(pages.len(), 1);
    assert!(pages[0].starts_with("https://github.com/settings/personal-access-tokens/new?"));
    assert_eq!(ctx.opener.forms()[0].name, "m1-github-issues");
}

#[test]
fn issues_token_fix_delivers_when_the_vault_has_it() {
    let machine = mac();
    let runner = Arc::new(ScriptedRunner::new());
    let ctx = Ctx::new(machine.clone())
        .runner(&runner)
        .secrets(&[("m1-github-issues", "github_pat_ABC")]);
    let step = issues_token(&machine, machine.sd_worker().unwrap());
    let StepFix::Run(plan) = &step.fix else {
        panic!("not a Fix");
    };
    let (fix, _) = ctx.fix(None);
    let outcome = (plan.run)(&fix);
    assert!(outcome.ok, "{}", outcome.message);
    assert!(ctx.opener.pages().is_empty() && ctx.opener.forms().is_empty());
    assert_eq!(runner.calls()[0].stdin_text(), "github_pat_ABC");
}

#[test]
fn the_probes_of_both_steps_look_at_their_file() {
    let machine = TestMachines::linux("vps-sing-1");
    let claude = claude_login(&machine);
    let issues = issues_token(&machine, machine.sd_worker().unwrap());
    let (claude_script, issues_script) = (
        claude.probe.script().unwrap().to_string(),
        issues.probe.script().unwrap().to_string(),
    );
    assert!(claude_script.starts_with(&shell::stat(&[shell::CLAUDE_ENV], true)));
    assert!(issues_script.starts_with(&shell::stat(&[shell::GITHUB_TOKEN], true)));
    assert!(claude_script.contains("sha256sum") && issues_script.contains("sha256sum"));
}

#[test]
fn send_again_shows_the_placeholder_and_never_a_value() {
    let shown = send_display(&claude_usage("m1"), &mac());
    assert!(shown.starts_with("Send the vault value of claude-worker to m1 again:\n"));
    assert!(shown.contains("printf 'CLAUDE_CODE_OAUTH_TOKEN=%s\\n' '[secret:claude-worker]' | "));
    assert!(shown.contains("/usr/bin/ssh"));
    let raw = send_display(&raw_usage(), &mac());
    assert!(raw.contains("printf %s '[secret:m1-github-issues]' | "));
}

// MARK: Token creation

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00");
            out.push(u8::from_str_radix(hex, 16).unwrap_or(b'?'));
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn repos(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn token_page_url() {
    let url = token_creation::page_url(
        "m1-github-issues",
        "m1",
        &repos(&["dtthaison/lingeva-mono"]),
    );
    let (base, query) = url.split_once('?').unwrap();
    assert_eq!(
        base,
        "https://github.com/settings/personal-access-tokens/new"
    );
    let items: Vec<(String, String)> = query
        .split('&')
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap();
            (k.to_string(), percent_decode(v))
        })
        .collect();
    let want = [
        ("name", "m1-github-issues"),
        ("description", "Issues only, for the sd worker on m1"),
        ("target_name", "dtthaison"),
        ("expires_in", "366"),
        ("issues", "write"),
    ];
    assert_eq!(
        items,
        want.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<Vec<_>>()
    );
}

/// A machine or secret name can't add a parameter or end the query.
#[test]
fn token_page_url_encodes_what_the_names_hold() {
    let url = token_creation::page_url("a&issues=admin#x", "m 1", &repos(&["o/n"]));
    let query = url.split_once('?').unwrap().1;
    assert!(!query.contains('#'));
    assert_eq!(query.matches('&').count(), 4, "{query}");
    assert!(query.starts_with("name=a%26issues%3Dadmin%23x&"), "{query}");
}

#[test]
fn token_clicks_name_only_select_repositories() {
    let clicks = token_creation::clicks(
        &repos(&["dtthaison/lingeva-mono", "dtthaison/other"]),
        false,
    );
    assert_eq!(
        clicks,
        vec![
            "Repository access: choose Only select repositories, then pick dtthaison/lingeva-mono, dtthaison/other.",
            "Permissions: Issues Read and write. Leave Contents at No access.",
            "Generate token, copy it, and delete any old token for this machine on the same page.",
            "Paste it into Notchling's form and Save.",
        ]
    );
    assert!(!clicks.join("").contains("All repositories"));
    assert!(token_creation::clicks(&repos(&["a/b"]), true)[0].contains("All repositories"));
}

#[test]
fn token_request_prefills_how_and_expires() {
    let today = expiry::day_number("2026-11-06").unwrap();
    let list = repos(&["dtthaison/lingeva-mono"]);
    let request = token_creation::request("m1-github-issues", "m1", &list, false, today);
    assert_eq!(request.name, "m1-github-issues");
    assert_eq!(request.kind.as_deref(), Some("token"));
    assert_eq!(request.url.as_deref(), Some("https://github.com"));
    assert_eq!(
        request.note.as_deref(),
        Some("Issues-only token for the sd worker on m1: dtthaison/lingeva-mono")
    );
    assert_eq!(request.expires.as_deref(), Some("2027-11-07"));
    assert_eq!(
        request.reason.as_deref(),
        Some("Machines: the GitHub issues token for m1")
    );
    let how = request.how_to.unwrap();
    assert!(how.contains(&token_creation::page_url("m1-github-issues", "m1", &list)));
    for click in token_creation::clicks(&list, false) {
        assert!(how.contains(&click), "{click}");
    }
}

#[test]
fn start_opens_the_page_and_the_form() {
    let opener = RecordingOpener::default();
    token_creation::start(
        &opener,
        "m1-github-issues",
        "m1",
        &repos(&["dtthaison/lingeva-mono"]),
        false,
        1_800_000_000.0,
    );
    assert_eq!(opener.pages().len(), 1);
    assert_eq!(opener.forms().len(), 1);
    assert!(opener.pages()[0].starts_with("https://github.com/"));
    assert_eq!(opener.forms()[0].name, "m1-github-issues");
}

// MARK: Renames

fn entry(name: &str, kind: Kind) -> VaultEntry {
    VaultEntry::new(name, kind)
}

#[test]
fn legacy_renames() {
    let list = vec![
        entry("sd-worker-m1", Kind::Token),
        entry("sd-claude", Kind::Token),
        entry("claude-worker", Kind::Token),
        entry("other", Kind::Token),
    ];
    let renames = pending_renames(&list);
    assert_eq!(
        renames
            .iter()
            .map(|r| r.0.name.as_str())
            .collect::<Vec<_>>(),
        vec!["sd-worker-m1"]
    );
    assert_eq!(
        renames.iter().map(|r| r.1.as_str()).collect::<Vec<_>>(),
        vec!["m1-github-issues"]
    );
    assert_eq!(
        LEGACY_NAMES.iter().map(|p| p.0).collect::<Vec<_>>(),
        vec!["sd-worker-m1", "sd-worker-vps-sing-1", "sd-claude"]
    );
    assert_eq!(
        LEGACY_NAMES.iter().map(|p| p.1).collect::<Vec<_>>(),
        vec![
            "m1-github-issues",
            "vps-sing-1-github-issues",
            "claude-worker"
        ]
    );
    assert!(pending_renames(&[]).is_empty());
    // names match without regard to case
    let upper = vec![entry("SD-Claude", Kind::Token)];
    assert_eq!(pending_renames(&upper).len(), 1);
}

#[test]
fn renamed_entry_variables_follow_the_new_name() {
    let renamed = entry("sd-worker-m1", Kind::Token).renamed("m1-github-issues");
    assert_eq!(renamed.name, "m1-github-issues");
    assert_eq!(renamed.secret_var, "M1_GITHUB_ISSUES_TOKEN");
    let mut custom = entry("sd-worker-m1", Kind::Token);
    custom.secret_var = "GH_TOKEN".into();
    assert_eq!(custom.renamed("m1-github-issues").secret_var, "GH_TOKEN");
    let login = entry("old-login", Kind::Login).renamed("new-login");
    assert_eq!(login.user_var, "NEW_LOGIN_USERNAME");
    assert_eq!(login.secret_var, "NEW_LOGIN_PASSWORD");
}

#[test]
fn renamed_entry_keeps_everything_else() {
    let mut old = entry("sd-claude", Kind::Token);
    old.note = "n".into();
    old.url = Some("https://claude.ai".into());
    old.how_to = Some("h".into());
    old.expires = Some("2027-01-01".into());
    old.ask = Some(true);
    let renamed = old.renamed("claude-worker");
    assert_eq!(renamed.note, "n");
    assert_eq!(renamed.url.as_deref(), Some("https://claude.ai"));
    assert_eq!(renamed.how_to.as_deref(), Some("h"));
    assert_eq!(renamed.expires.as_deref(), Some("2027-01-01"));
    assert_eq!(renamed.ask, Some(true));
    assert_eq!(renamed.updated, old.updated);
}

// MARK: Vault as a source

#[test]
fn vault_secret_source_asks_its_injected_index_and_store() {
    let known = entry("claude-worker", Kind::Token);
    let for_lookup = known.clone();
    let source = VaultSecretSource::new(
        move |name| (name == "claude-worker").then(|| for_lookup.clone()),
        |e| {
            (e.name == "claude-worker").then(|| VaultValue {
                username: String::new(),
                secret: "tok".into(),
            })
        },
    );
    assert_eq!(source.value("claude-worker").unwrap().expose(), "tok");
    assert_eq!(source.entry("claude-worker"), Some(known));
    assert!(source.value("other").is_none());
    assert!(source.entry("other").is_none());
}
