//! Tests of the repo steps' names, Deploy key Check and Fix, Clone and Push is refused (Swift
//! `MachinesRepoStepsTests`, first half). Replace key is `tests_repo_replace`.

use super::fakes_b::*;
use super::registry::{Machine, MachineRepo};
use super::repo_common::*;
use super::step::{CheckResult, CheckState, FixOutcome, Probe, RemoteOutput, StepFix, StepId};
use super::steps_repo;

pub(super) const ALIAS: &str = "github-agents-dtthaison-lingeva-mono";

pub(super) fn steps(m: &Machine) -> Vec<super::step::SetupStep> {
    steps_repo::all(m, &m.repos()[0].clone())
}

pub(super) fn probe(id: &StepId, m: &Machine) -> (String, super::step::RemoteParse) {
    remote(&find(&steps(m), id))
}

pub(super) fn fix_plan(id: &StepId, m: &Machine) -> super::step::FixPlan {
    plan(&find(&steps(m), id))
}

pub(super) fn remote_text(pubkey: &str, alias_line: &str, ssh: &str) -> String {
    format!("{pubkey}\n@@config\n{alias_line}\n@@ssh\n{ssh}\n")
}

/// Runs the Deploy key Check on `text` with GitHub listing `keys`.
pub(super) fn check(text: &str, keys: &str) -> (CheckResult, Rig) {
    let rig = Rig::new(mac("m1"));
    rig.runner.on(is_list, ok(keys));
    let (_, parse) = probe(&StepId::deploy_key(REPO), &rig.machine);
    let result = parse(
        &RemoteOutput {
            text: text.into(),
            code: 0,
        },
        &rig.step(),
    );
    (result, rig)
}

pub(super) fn fail_text(r: &CheckResult) -> String {
    match &r.state {
        CheckState::Fail(t) => t.clone(),
        other => panic!("not a fail: {other:?}"),
    }
}

// MARK: Names

#[test]
fn alias_key_and_remote_names() {
    assert_eq!(slug("dtthaison/lingeva-mono"), "dtthaison-lingeva-mono");
    assert_eq!(slug("Me/My.App"), "me-my-app");
    assert_eq!(alias(REPO), ALIAS);
    assert_eq!(key_file(REPO), "~/.ssh/agents-dtthaison-lingeva-mono");
    assert_eq!(
        remote_url(REPO),
        format!("git@{ALIAS}:dtthaison/lingeva-mono.git")
    );
}

#[test]
fn repo_names_follow_githubs_rules() {
    for ok in [
        "dtthaison/lingeva-mono",
        "org/.github",
        "org/_x",
        "a/b.c_d-e",
        "Me/My.App",
        "org/...x",
    ] {
        assert!(is_valid_repo(ok), "{ok}");
    }
    for bad in [
        "org/.",
        "org/..",
        "org/-x",
        "-org/x",
        ".org/x",
        "a/b/c",
        "a/b; touch x",
        "a",
        "/b",
        "a/",
        "a/b\nc",
        "a/$(id)",
    ] {
        assert!(!is_valid_repo(bad), "{bad}");
    }
}

#[test]
fn machine_names_for_titles_and_paths() {
    for ok in ["m1", "vps-sing-1", "a.b_c", &"a".repeat(64)] {
        assert!(is_valid_machine_name(ok), "{ok}");
    }
    for bad in ["", "-m", "m 1", "m1; touch x", "m1\n", &"a".repeat(65)] {
        assert!(!is_valid_machine_name(bad), "{bad}");
    }
}

#[cfg(unix)]
#[test]
fn the_alias_guard_matches_indented_and_padded_host_lines_only() {
    let pattern = alias_pattern(ALIAS);
    let dir = TempDir::new("alias");
    let file = dir.path().join("config");
    let matches = |line: &str| {
        std::fs::write(&file, format!("{line}\n")).unwrap();
        std::process::Command::new("/usr/bin/grep")
            .args(["-Eq", &pattern])
            .arg(&file)
            .status()
            .unwrap()
            .success()
    };
    for line in [
        format!("Host {ALIAS}"),
        format!("  Host {ALIAS}"),
        format!("\tHost  {ALIAS}"),
        format!("Host {ALIAS}  "),
        format!("Host {ALIAS} other"),
    ] {
        assert!(matches(&line), "{line}");
    }
    for line in [
        format!("Host {ALIAS}-2"),
        "Host other".to_string(),
        format!("# Host {ALIAS}"),
        format!("Hostname {ALIAS}"),
        format!("Host x{ALIAS}"),
    ] {
        assert!(!matches(&line), "{line}");
    }
}

#[test]
fn the_checkout_defaults_to_src_name() {
    let mut r = MachineRepo {
        github: REPO.into(),
        local_path: "/x".into(),
        check_command: "true".into(),
        machine_path: None,
        verify_skill: None,
    };
    assert_eq!(checkout_path(&r), "~/src/lingeva-mono");
    r.machine_path = Some("/srv/mono".into());
    assert_eq!(checkout_path(&r), "/srv/mono");
}

// MARK: Deploy key Check

#[test]
fn deploy_key_passes_only_when_github_lists_it_read_only_and_ssh_t_works() {
    let (good, rig) = check(
        &remote_text(&pub_line(), "alias=yes", WELCOME),
        &gh_keys(&format!("ssh-ed25519 {BODY}"), Some(true)),
    );
    assert_eq!(
        good.state,
        CheckState::Pass(format!("read-only key on {REPO}"))
    );
    assert_eq!(rig.argvs(), vec![list_argv(REPO)]);

    let (writable, _) = check(
        &remote_text(&pub_line(), "alias=yes", WELCOME),
        &gh_keys(&format!("ssh-ed25519 {BODY}"), Some(false)),
    );
    assert!(fail_text(&writable).contains("can write"));

    let (absent, _) = check(
        &remote_text(&pub_line(), "alias=yes", WELCOME),
        &gh_keys(&format!("ssh-ed25519 {OTHER_BODY}"), Some(true)),
    );
    assert!(fail_text(&absent).contains("not on dtthaison/lingeva-mono yet"));

    let (denied, _) = check(
        &remote_text(
            &pub_line(),
            "alias=yes",
            "git@github.com: Permission denied (publickey).",
        ),
        &gh_keys(&format!("ssh-ed25519 {BODY}"), Some(true)),
    );
    assert!(denied.state.is_fail());
}

#[test]
fn a_key_without_read_only_field_is_not_trusted_as_read_only() {
    let (result, _) = check(
        &remote_text(&pub_line(), "alias=yes", WELCOME),
        &gh_keys(&format!("ssh-ed25519 {BODY}"), None),
    );
    assert!(fail_text(&result).contains("can write"));
}

#[test]
fn a_missing_alias_or_missing_key_file_fails() {
    let keys = gh_keys(&format!("ssh-ed25519 {BODY}"), Some(true));
    let (no_alias, _) = check(&remote_text(&pub_line(), "alias=no", WELCOME), &keys);
    assert!(no_alias.state.is_fail());
    let (no_key, rig) = check(&remote_text("", "alias=no", "x"), &keys);
    assert!(fail_text(&no_key).contains("no deploy key"));
    assert!(rig.calls().is_empty());
}

#[test]
fn gh_failing_is_cant_tell_not_fail() {
    let rig = Rig::new(mac("m1"));
    rig.runner
        .on(is_list, result(1, "", "gh: Not Found (HTTP 404)"));
    let (_, parse) = probe(&StepId::deploy_key(REPO), &rig.machine);
    let r = parse(
        &RemoteOutput {
            text: remote_text(&pub_line(), "alias=yes", WELCOME),
            code: 0,
        },
        &rig.step(),
    );
    assert!(matches!(r.state, CheckState::CantTell(_)), "{:?}", r.state);
}

#[test]
fn a_public_key_that_is_not_one_ed25519_line_is_refused() {
    let cases = [
        "ssh-ed25519 AAAA$(touch x)".to_string(),
        format!("{}\nssh-ed25519 {OTHER_BODY}", pub_line()),
        format!("ssh-rsa {BODY} agents-m1"),
        format!("ssh-ed25519 {BODY} x$(id)"),
        format!("ssh-ed25519 {BODY} a;b"),
        format!("ssh-ed25519 {BODY}\nrm -rf x"),
        // a key file that holds marker lines must not fake the later parts
        format!("ssh-ed25519 {BODY}\n@@config\nalias=yes\n@@ssh\n{WELCOME}"),
    ];
    for text in cases {
        let rig = Rig::new(mac("m1"));
        let full = if text.contains("@@config") {
            format!("{text}\n@@config\nalias=no\n@@ssh\nPermission denied")
        } else {
            remote_text(&text, "alias=yes", WELCOME)
        };
        let (_, parse) = probe(&StepId::deploy_key(REPO), &rig.machine);
        let r = parse(
            &RemoteOutput {
                text: full,
                code: 0,
            },
            &rig.step(),
        );
        let why = fail_text(&r);
        assert!(why.contains("unexpected public key"), "{text}: {why}");
        assert!(rig.calls().is_empty(), "gh was called for: {text}");
        assert!(!why.contains("touch"), "{why}");
    }
}

#[test]
fn the_probe_only_reads_and_asks_github_with_ssh_t() {
    let (script, _) = probe(&StepId::deploy_key(REPO), &mac("m1"));
    assert!(script.contains("head -c 1024"));
    assert!(script.contains(&format!(
        "ssh -T -o BatchMode=yes -o ConnectTimeout=8 {ALIAS} </dev/null"
    )));
    assert!(script.contains(&format!("grep -Eq '{}'", alias_pattern(ALIAS))));
    assert!(!script.contains("ssh-keygen"));
    assert!(!script.contains(">>"));
    // the private key is never printed
    assert!(!script.contains("cat "));
}

// MARK: Deploy key Fix

const GH_META: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQCj7ndNxQowgcQnjshcLrqPEiiphnt+VTTvDP6mHBL9j1aNUkY4Ue1gvwnGLVlOhGeYrnZaMgRK6\n";

fn fix_rig(scanned: Option<&str>, add_code: i32, on_github: &str) -> Rig {
    let rig = Rig::new(mac("m1"));
    rig.runner.on(is_list, ok(on_github));
    rig.runner
        .on(script_has("ssh-keygen"), ok(&format!("{}\n", pub_line())));
    rig.runner.on(
        |a| a.iter().any(|s| s == "meta"),
        ok(&format!("{GH_META}ssh-ed25519 {GITHUB_HOST_BODY}\n")),
    );
    let scanned = scanned
        .map(str::to_string)
        .unwrap_or_else(|| format!("github.com ssh-ed25519 {GITHUB_HOST_BODY}\n"));
    rig.runner.on(script_has("ssh-keyscan"), ok(&scanned));
    rig.runner.on(
        |a| a.iter().any(|s| s == "deploy-key"),
        result(add_code, "", if add_code == 0 { "" } else { "boom" }),
    );
    rig
}

fn run_deploy(rig: &Rig) -> FixOutcome {
    let p = fix_plan(&StepId::deploy_key(REPO), &rig.machine);
    (p.run)(&rig.fix())
}

fn mentions_add(argvs: &[Vec<String>]) -> bool {
    argvs.iter().any(|a| a.iter().any(|s| s == "deploy-key"))
}

#[test]
fn deploy_key_fix_order() {
    let rig = fix_rig(None, 0, "[]");
    let outcome = run_deploy(&rig);
    assert!(outcome.ok, "{}", outcome.message);
    let calls = rig.argvs();
    assert_eq!(calls.len(), 6);
    assert_eq!(calls[0][0], "/usr/bin/ssh");
    let keygen = unquoted(last(&calls[0]));
    assert!(keygen.contains("ssh-keygen -q -t ed25519 -N ''"));
    assert!(keygen.contains("-C 'agents-m1'"));
    assert!(last(&calls[0]).contains(&format!("Host {ALIAS}")));
    assert!(last(&calls[0]).contains("IdentityFile ~/.ssh/agents-dtthaison-lingeva-mono"));
    assert_eq!(
        calls[1],
        vec![
            gh(),
            "api".into(),
            "meta".into(),
            "--jq".into(),
            ".ssh_keys[]".into()
        ]
    );
    assert_eq!(calls[2][0], "/usr/bin/ssh");
    assert!(last(&calls[2]).contains("ssh-keyscan -t ed25519 github.com 2>/dev/null"));
    assert_eq!(calls[3][0], "/usr/bin/ssh");
    assert!(last(&calls[3]).contains("known_hosts"));
    assert!(last(&calls[3]).contains(&format!("github.com ssh-ed25519 {GITHUB_HOST_BODY}")));
    assert!(!last(&calls[3]).contains(OTHER_BODY));
    // a key GitHub does not list yet is added, after the list call
    assert_eq!(calls[4], list_argv(REPO));
    assert_eq!(
        &calls[5][..4],
        &[gh(), "repo".into(), "deploy-key".into(), "add".into()]
    );
    assert!(
        calls[5][4].ends_with("m1-dtthaison-lingeva-mono.pub"),
        "{}",
        calls[5][4]
    );
    assert_eq!(
        &calls[5][calls[5].len() - 4..],
        &["-R", REPO, "--title", "agents-m1"]
    );
    assert_eq!(calls[5].len(), 9);
    // the public key file is gone again
    assert!(!std::path::Path::new(&calls[5][4]).exists());
}

#[test]
fn a_key_already_listed_read_only_is_not_added_again() {
    let rig = fix_rig(
        None,
        0,
        &gh_keys(&format!("ssh-ed25519 {BODY}"), Some(true)),
    );
    let outcome = run_deploy(&rig);
    assert!(outcome.ok, "{}", outcome.message);
    let calls = rig.argvs();
    assert!(!mentions_add(&calls));
    // the machine was still repaired first
    assert!(last(&calls[0]).contains("ssh-keygen"));
    assert!(calls.iter().any(|a| last(a).contains("known_hosts")));
}

#[test]
fn a_key_already_listed_as_writable_stops_without_adding() {
    let rig = fix_rig(
        None,
        0,
        &gh_keys(&format!("ssh-ed25519 {BODY}"), Some(false)),
    );
    let outcome = run_deploy(&rig);
    assert!(!outcome.ok);
    assert!(outcome.message.contains("can write"));
    assert!(outcome.message.contains("Settings > Deploy keys"));
    assert!(!mentions_add(&rig.argvs()));
}

#[test]
fn another_key_on_github_still_gets_ours_added() {
    let rig = fix_rig(
        None,
        0,
        &gh_keys(&format!("ssh-ed25519 {OTHER_BODY}"), Some(true)),
    );
    assert!(run_deploy(&rig).ok);
    assert!(mentions_add(&rig.argvs()));
}

#[test]
fn a_list_that_cannot_be_read_adds_nothing() {
    let rig = fix_rig(None, 0, "not json");
    assert!(!run_deploy(&rig).ok);
    assert!(!mentions_add(&rig.argvs()));
}

#[test]
fn the_write_flag_can_never_appear() {
    let rig = fix_rig(None, 0, "[]");
    run_deploy(&rig);
    for argv in rig.argvs() {
        assert!(!argv.iter().any(|a| a.contains("allow-write")), "{argv:?}");
        assert!(!argv.iter().any(|a| a == "-w"), "{argv:?}");
    }
    assert!(!fix_plan(&StepId::deploy_key(REPO), &rig.machine)
        .display
        .contains("allow-write"));
    // and not in the source either
    for source in [
        include_str!("steps_repo.rs"),
        include_str!("steps_replace.rs"),
        include_str!("steps_push.rs"),
        include_str!("repo_common.rs"),
    ] {
        assert!(!source.contains("allow-write"));
    }
}

#[test]
fn only_the_public_key_is_ever_read_from_the_machine() {
    let rig = fix_rig(None, 0, "[]");
    run_deploy(&rig);
    for argv in rig.argvs().iter().filter(|a| a[0] == "/usr/bin/ssh") {
        for line in unquoted(last(argv))
            .split('\n')
            .filter(|l| l.starts_with("cat "))
        {
            assert!(line.contains(".pub'"), "{line}");
        }
    }
    assert!(rig.calls().iter().all(|c| c.stdin.is_none()));
}

#[test]
fn a_host_key_mismatch_stops_before_anything_is_added() {
    let rig = fix_rig(
        Some(&format!("github.com ssh-ed25519 {OTHER_BODY}\n")),
        0,
        "[]",
    );
    let outcome = run_deploy(&rig);
    assert!(!outcome.ok);
    assert!(outcome.message.contains("differs from gh api meta"));
    let calls = rig.argvs();
    assert!(!calls.iter().any(|a| last(a).contains("known_hosts")));
    assert!(!mentions_add(&calls));
}

#[test]
fn a_keyscan_that_sees_nothing_adds_nothing() {
    let rig = fix_rig(Some(""), 0, "[]");
    assert!(!run_deploy(&rig).ok);
    assert!(!mentions_add(&rig.argvs()));
}

#[test]
fn an_unexpected_key_from_the_machine_is_never_sent_to_github() {
    let rig = Rig::new(mac("m1"));
    rig.runner
        .on(script_has("ssh-keygen"), ok("ssh-rsa AAAA\nrm -rf /\n"));
    let outcome = run_deploy(&rig);
    assert!(!outcome.ok);
    assert!(outcome.message.contains("unexpected public key"));
    assert_eq!(rig.calls().len(), 1);
}

#[test]
fn a_failed_add_is_reported_with_ghs_first_line() {
    let rig = fix_rig(None, 1, "[]");
    let outcome = run_deploy(&rig);
    assert!(!outcome.ok);
    assert!(outcome.message.contains("boom"));
}

#[test]
fn an_unsafe_machine_name_runs_nothing() {
    let mut m = mac("m1");
    m.name = "m1; touch x".into();
    let rig = fix_rig(None, 0, "[]");
    let rig = Rig {
        machine: m.clone(),
        all: vec![m],
        ..rig
    };
    let outcome = run_deploy(&rig);
    assert!(!outcome.ok);
    assert!(rig.calls().is_empty());
}

#[test]
fn an_unsafe_repo_name_gets_no_scripts() {
    let m = mac("m1");
    let bad = MachineRepo {
        github: "a/b; touch x".into(),
        local_path: "/x".into(),
        check_command: "true".into(),
        machine_path: None,
        verify_skill: None,
    };
    let all = steps_repo::all(&m, &bad);
    assert_eq!(all.len(), 3);
    for step in all {
        assert!(matches!(step.probe, Probe::Local(_)), "{}", step.id.raw());
        assert!(matches!(step.fix, StepFix::HowTo(_)), "{}", step.id.raw());
        assert!(step.replace.is_none());
    }
}

// MARK: Clone

#[test]
fn clone_uses_the_alias_and_refuses_another_checkout() {
    let id = StepId::clone_repo(REPO);
    let m = mac("m1");
    let (script, parse) = probe(&id, &m);
    assert!(
        script.contains(
            "git -C \"$HOME\"/'src/lingeva-mono' remote get-url origin 2>/dev/null || echo none"
        ),
        "{script}"
    );
    let rig = Rig::new(m.clone());
    let run = |text: &str| {
        parse(
            &RemoteOutput {
                text: text.into(),
                code: 0,
            },
            &rig.step(),
        )
    };
    assert_eq!(run("none\n").state, CheckState::Fail("not cloned".into()));
    let other = run("git@github.com:someone/else.git\n");
    assert!(fail_text(&other).contains("git@github.com:someone/else.git"));
    let same = run(&format!("git@{ALIAS}:dtthaison/lingeva-mono.git\n"));
    assert_eq!(same.state, CheckState::Pass("~/src/lingeva-mono".into()));

    let fix = fix_plan(&id, &m);
    assert!(fix.display.contains(&format!(
        "git clone -- git@{ALIAS}:dtthaison/lingeva-mono.git"
    )));
    assert!(fix.display.contains(
        "if [ -e \"$p\" ]; then echo \"$p already exists; Notchling won't touch it\" >&2; exit 1; fi"
    ));
    assert!(!fix.needs_admin);

    let outcome = (fix.run)(&rig.fix());
    assert!(outcome.ok);
    let calls = rig.argvs();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0][0], "/usr/bin/ssh");
    assert!(last(&calls[0]).contains(&format!(
        "git clone -- git@{ALIAS}:dtthaison/lingeva-mono.git"
    )));
}

#[test]
fn clone_refuses_an_existing_folder_with_the_machines_message() {
    let rig = Rig::new(mac("m1"));
    rig.runner.on(
        |_| true,
        result(
            1,
            "",
            "/home/agent/src/lingeva-mono already exists; Notchling won't touch it\n",
        ),
    );
    let outcome = (fix_plan(&StepId::clone_repo(REPO), &rig.machine).run)(&rig.fix());
    assert!(!outcome.ok);
    assert!(outcome.message.contains("already exists"));
}

#[test]
fn a_machine_path_with_shell_characters_is_quoted() {
    let mut m = mac("m1");
    let mut r = m.repos()[0].clone();
    r.machine_path = Some("/srv/a b'; touch x; '".into());
    m.sd_worker_mut().unwrap().repos = vec![r.clone()];
    let step = find(&steps_repo::all(&m, &r), &StepId::clone_repo(REPO));
    let (script, _) = remote(&step);
    assert!(
        script.contains(&crate::lanes::quote("/srv/a b'; touch x; '")),
        "{script}"
    );
}
