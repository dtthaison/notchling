use super::fakes_b::*;
use super::registry::Machine;
use super::step::{CheckState, RemoteOutput, StepFix, StepId};
use super::tests_repo::{fail_text, fix_plan, probe, remote_text, steps};

fn push_output(git: &str, code: i32, checks: &[&str]) -> RemoteOutput {
    let mut lines = vec![git.to_string(), format!("@@push {code}")];
    lines.extend(checks.iter().map(|c| c.to_string()));
    RemoteOutput {
        text: lines.join("\n") + "\n",
        code: 0,
    }
}

const QUIET: [&str; 4] = ["gh=1", "ssh=denied", "keychain=44", "credfile=1"];

fn push_parse(m: &Machine) -> (String, super::step::RemoteParse) {
    probe(&StepId::push_refused(REPO), m)
}

#[test]
fn push_refused_needs_githubs_read_only_message() {
    let rig = Rig::new(mac("m1"));
    let (script, parse) = push_parse(&rig.machine);
    assert!(script.contains("push --dry-run origin HEAD:refs/heads/agents-push-test"));
    let ctx = rig.step();
    let refused = parse(&push_output(READ_ONLY_MSG, 128, &QUIET), &ctx);
    assert_eq!(
        refused.state,
        CheckState::Pass(
            "GitHub refuses a push with this key; no other GitHub login on this account".into()
        )
    );
    let accepted = parse(
        &push_output(
            "To github:x\n * [new branch] HEAD -> agents-push-test",
            0,
            &QUIET,
        ),
        &ctx,
    );
    assert!(fail_text(&accepted).contains("can push"));
    let other = parse(
        &push_output("fatal: not a git repository\nmore", 128, &QUIET),
        &ctx,
    );
    assert_eq!(
        other.state,
        CheckState::Fail("fatal: not a git repository".into())
    );
    let cut = parse(
        &RemoteOutput {
            text: "fatal: something\n".into(),
            code: 1,
        },
        &ctx,
    );
    assert_eq!(cut.state, CheckState::Fail("fatal: something".into()));
}

#[test]
fn push_refused_fails_when_the_worker_has_its_own_github_login() {
    let rig = Rig::new(mac("m1"));
    let (_, parse) = push_parse(&rig.machine);
    let ctx = rig.step();
    let cases = [
        ("gh=0", "gh is logged in as the worker; run gh auth logout"),
        (
            "ssh=hi:sondoan",
            "a default ssh key logs in to GitHub as sondoan",
        ),
        ("keychain=0", "the keychain holds a github.com password"),
        ("credfile=0", "~/.git-credentials holds a github.com login"),
    ];
    for (check, why) in cases {
        let name = check.split('=').next().unwrap();
        let mut checks = QUIET.to_vec();
        let i = checks
            .iter()
            .position(|c| c.starts_with(&format!("{name}=")))
            .unwrap();
        checks[i] = check;
        let result = parse(&push_output(READ_ONLY_MSG, 128, &checks), &ctx);
        assert_eq!(result.state, CheckState::Fail(why.into()), "{check}");
        assert!(result.rows.iter().any(|r| !r.ok), "{check}");
    }
    let both = parse(
        &push_output(
            READ_ONLY_MSG,
            128,
            &["gh=0", "ssh=denied", "keychain=0", "credfile=1"],
        ),
        &ctx,
    );
    assert_eq!(
        both.state,
        CheckState::Fail(
            "gh is logged in as the worker; run gh auth logout; the keychain holds a github.com password".into()
        )
    );
}

#[test]
fn an_ssh_key_that_answers_for_a_repo_or_not_at_all_is_not_a_login() {
    let rig = Rig::new(mac("m1"));
    let (_, parse) = push_parse(&rig.machine);
    let ctx = rig.step();
    let run = |checks: [&str; 4]| parse(&push_output(READ_ONLY_MSG, 128, &checks), &ctx);
    let deploy = run([
        "gh=1",
        "ssh=hi:dtthaison/lingeva-mono",
        "keychain=44",
        "credfile=1",
    ]);
    assert!(matches!(deploy.state, CheckState::Pass(_)));
    let unknown = run(["gh=1", "ssh=unknown", "keychain=44", "credfile=1"]);
    assert!(matches!(unknown.state, CheckState::CantTell(_)));
    let locked = run(["gh=1", "ssh=denied", "keychain=36", "credfile=1"]);
    assert!(matches!(locked.state, CheckState::CantTell(_)));
    let no_gh = run(["gh=none", "ssh=denied", "keychain=44", "credfile=1"]);
    assert!(matches!(no_gh.state, CheckState::Pass(_)));
    let odd = run(["gh=1", "ssh=hi:$(touch x)", "keychain=44", "credfile=1"]);
    assert_eq!(
        odd.state,
        CheckState::Fail("a default ssh key logs in to GitHub".into())
    );
    assert!(!format!("{:?} {:?}", odd.state, odd.rows).contains("touch"));
}

#[test]
fn the_login_checks_keep_no_output_and_write_nothing() {
    for m in [mac("m1"), linux("vps")] {
        let (script, _) = push_parse(&m);
        assert!(script.contains("gh auth status --hostname github.com >/dev/null 2>&1"));
        assert!(script.contains(
            "ssh -o BatchMode=yes -o ConnectTimeout=8 -o StrictHostKeyChecking=yes -T git@github.com"
        ));
        assert!(script.contains("grep -q 'github.com' ~/.git-credentials 2>/dev/null"));
        assert!(
            !script.contains("gh auth token"),
            "the token is never printed"
        );
        assert!(
            !(script.contains("gh auth login")
                || script.contains("gh auth logout")
                || script.contains("gh auth setup-git"))
        );
        assert!(!(script.contains("credential fill") || script.contains("credential approve")));
        assert!(!(script.contains(" -g") || script.contains(" -w")));
        assert!(!(script.contains("accept-new") || script.contains("StrictHostKeyChecking=no")));
        assert_eq!(
            script.split(" push ").count(),
            2,
            "the only push is the dry run"
        );
    }
    assert!(push_parse(&mac("m1"))
        .0
        .contains("security find-internet-password -s github.com >/dev/null 2>&1"));
    assert!(
        !push_parse(&linux("vps")).0.contains("security "),
        "no keychain on Linux"
    );
}

#[cfg(unix)]
#[test]
fn the_login_checks_run_to_token_lines_and_never_print_the_tools_output() {
    let dir = TempDir::new("login");
    let bin = dir.path().join("bin");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let stub = |name: &str, body: &str| {
        use std::os::unix::fs::PermissionsExt;
        let p = bin.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    stub(
        "gh",
        "echo 'Logged in to github.com account sondoan (token gho_SECRETSECRET)'; exit 0",
    );
    stub("ssh", "echo 'Hi sondoan! You have successfully authenticated, but GitHub does not provide shell access.' >&2; exit 1");
    stub(
        "security",
        "echo 'keychain: \"srvr\"<blob>=\"github.com\" password: SECRETSECRET'; exit 0",
    );
    stub("git", "echo 'remote: ERROR: The key you are authenticating with has been marked as read only.' >&2; exit 128");
    std::fs::write(
        home.join(".git-credentials"),
        "https://sondoan:SECRETSECRET@github.com\n",
    )
    .unwrap();
    let (script, _) = push_parse(&mac("m1"));
    let out = std::process::Command::new("/bin/sh")
        .args(["-c", &script])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!text.contains("SECRETSECRET"), "{text}");
    assert!(!text.contains("gho_"), "{text}");
    assert!(!text.contains("blob"), "{text}");
    for token in [
        "@@push 128",
        "gh=0",
        "ssh=hi:sondoan",
        "keychain=0",
        "credfile=0",
    ] {
        assert!(text.contains(token), "{token} in:\n{text}");
    }
}

#[test]
fn push_refused_never_sends_a_push() {
    let m = mac("m1");
    let step = find(&steps(&m), &StepId::push_refused(REPO));
    let (script, _) = remote(&step);
    assert_eq!(script.split(" push ").count(), 2);
    assert!(script.contains("push --dry-run"));
    assert!(script.contains("git -c core.hooksPath=/dev/null -C"));
    assert!(script.contains("</dev/null 2>&1"));
    let StepFix::HowTo(text) = &step.fix else {
        panic!("not a how-to");
    };
    assert!(text.contains("Settings > Deploy keys"));
    assert_eq!(step.cadence, super::step::Cadence::Slow);
}

#[test]
fn machine_text_is_masked_before_it_is_cut_in_checks_and_fixes() {
    let token = long_token();
    let marker = "[secret:m1-github-issues]";
    let said = format!("{}{token}", "f".repeat(170));
    let rig = Rig::new(mac("m1")).hiding("m1-github-issues", &token);
    let m = rig.machine.clone();

    let (_, push) = push_parse(&m);
    let r = push(
        &RemoteOutput {
            text: format!("{said}\n"),
            code: 128,
        },
        &rig.step(),
    );
    let shown = format!("{:?}", r.state);
    assert_no_fragment(&token, &shown);
    assert!(shown.contains(marker), "{shown}");

    let (_, clone) = probe(&StepId::clone_repo(REPO), &m);
    let r = clone(
        &RemoteOutput {
            text: format!("{said}\n"),
            code: 0,
        },
        &rig.step(),
    );
    assert_no_fragment(&token, &format!("{:?}", r.state));

    let listing = Rig::new(mac("m1")).hiding("m1-github-issues", &token);
    listing.runner.on(is_list, result(1, "", &said));
    let (_, key) = probe(&StepId::deploy_key(REPO), &m);
    let r = key(
        &RemoteOutput {
            text: remote_text(&pub_line(), "alias=yes", WELCOME),
            code: 0,
        },
        &listing.step(),
    );
    assert_no_fragment(&token, &format!("{:?}", r.state));
    let ok_rig = Rig::new(mac("m1")).hiding("m1-github-issues", &token);
    ok_rig.runner.on(
        is_list,
        ok(&gh_keys(&format!("ssh-ed25519 {BODY}"), Some(true))),
    );
    let r = key(
        &RemoteOutput {
            text: remote_text(&pub_line(), "alias=yes", &said),
            code: 0,
        },
        &ok_rig.step(),
    );
    assert_no_fragment(&token, &format!("{:?} {:?}", r.state, r.rows));

    let failing = Rig::new(mac("m1")).hiding("m1-github-issues", &token);
    failing.runner.on(|_| true, result(128, "", &said));
    let cloned = (fix_plan(&StepId::clone_repo(REPO), &m).run)(&failing.fix());
    assert!(!cloned.ok);
    assert_no_fragment(&token, &cloned.message);
    assert!(cloned.message.contains(marker), "{}", cloned.message);
    let keyed = (fix_plan(&StepId::deploy_key(REPO), &m).run)(&failing.fix());
    assert_no_fragment(&token, &keyed.message);
}

#[test]
fn steps_keep_their_order_and_dependencies() {
    let all = steps(&mac("m1"));
    let ids: Vec<StepId> = all.iter().map(|s| s.id.clone()).collect();
    assert_eq!(
        ids,
        vec![
            StepId::deploy_key(REPO),
            StepId::clone_repo(REPO),
            StepId::push_refused(REPO)
        ]
    );
    assert_eq!(all[0].needs, vec![StepId::REACHABLE, StepId::BASE_TOOLS]);
    assert_eq!(all[1].needs, vec![StepId::deploy_key(REPO)]);
    assert_eq!(all[2].needs, vec![StepId::clone_repo(REPO)]);
}
