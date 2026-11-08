//! The window's pure pieces (Swift `MachinesWindowTests`, the forms and texts).

use super::forms::*;
use super::registry::{DockerState, MachineAbilities, MachineOs, MachineRepo, SdWorkerSettings};
use super::step::CheckResult;
use super::usage::{SecretFormat, SecretUsage};
use crate::vault::{expiry, Kind, VaultEntry};

#[test]
fn the_add_form_refuses_unsafe_names_hosts_and_users() {
    let p = |n: &str, h: &str, u: &str, e: &[&str]| {
        machine_problems(
            n,
            h,
            u,
            &e.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    };
    assert_eq!(p("M1", "m1", "u", &[]), vec![NAME_PROBLEM]);
    assert_eq!(p("m1", "m1", "u", &["m1"]), vec!["m1 is already a machine"]);
    assert_eq!(p("M1", "m1", "u", &["m1"]), vec![NAME_PROBLEM]);
    for bad in ["", "-m", "m1\n"] {
        assert_eq!(p(bad, "m1", "u", &[]), vec![NAME_PROBLEM], "{bad:?}");
    }
    for name in ["m1-20261001", "vps-sing-1-99999999", "a-00000000"] {
        assert_eq!(p(name, "m1", "u", &[]), vec![DATED_PROBLEM], "{name}");
    }
    for name in [
        "m1-2026100",
        "m1-202610011",
        "20261001",
        "m1-20261001-x",
        "m1-2026-1001",
    ] {
        assert!(p(name, "m1", "u", &[]).is_empty(), "{name}");
    }
    for host in [
        "-oProxyCommand=x",
        "a b",
        "h;x",
        "",
        "h\n",
        "a$(x)",
        ".a",
        "a/b",
    ] {
        assert_eq!(p("m2", host, "u", &[]), vec![HOST_PROBLEM], "{host:?}");
    }
    for host in [
        "m2.tail",
        "m2",
        "100.64.0.7",
        "fd7a:115c:a1e0::1",
        "vps-sing-1.tail1234.ts.net",
    ] {
        assert!(p("m2", host, "u", &[]).is_empty(), "{host}");
    }
    for user in ["root;x", "", "Root", "-x", "a b", "u\n"] {
        assert_eq!(p("m2", "m2", user, &[]), vec![USER_PROBLEM], "{user:?}");
    }
    assert!(p("m2", "m2.tail", "sondoan", &["m1"]).is_empty());
    assert_eq!(
        p("M", "-x", "", &[]),
        vec![NAME_PROBLEM, HOST_PROBLEM, USER_PROBLEM]
    );
}

#[test]
fn the_abilities_line() {
    let mac = MachineAbilities {
        arch: Some("arm64".into()),
        os_version: Some("26.1".into()),
        xcode: Some("26.1".into()),
        ios_runtimes: vec!["iOS 26.1".into()],
        simulators: vec!["iPhone 17".into(), "iPhone 17 Pro".into()],
        docker: Some(DockerState::Rootless),
        free_disk_gb: Some(120),
        ..MachineAbilities::default()
    };
    assert_eq!(
        abilities_line(Some(&mac), Some(MachineOs::MacOs)),
        "macOS 26.1 · arm64 · Xcode 26.1 · 2 simulators · Docker rootless · 120 GB free"
    );
    let linux = MachineAbilities {
        arch: Some("x86_64".into()),
        os_version: Some("Ubuntu 24.04.1 LTS".into()),
        docker: Some(DockerState::None),
        free_disk_gb: Some(80),
        ..MachineAbilities::default()
    };
    assert_eq!(
        abilities_line(Some(&linux), Some(MachineOs::Linux)),
        "Ubuntu 24.04.1 LTS · x86_64 · no Docker · 80 GB free"
    );
    let one = MachineAbilities {
        simulators: vec!["iPhone 17".into()],
        docker: Some(DockerState::Stopped),
        ..MachineAbilities::default()
    };
    assert_eq!(
        abilities_line(Some(&one), Some(MachineOs::MacOs)),
        "macOS · 1 simulator · Docker stopped"
    );
    assert_eq!(
        abilities_line(Some(&MachineAbilities::default()), Some(MachineOs::Linux)),
        "Linux"
    );
    assert_eq!(abilities_line(None, None), "not checked yet");
}

#[test]
fn update_offer_title_and_banners() {
    assert_eq!(update_offer_title(2), "Update 2 machines");
    assert_eq!(update_offer_title(1), "Update 1 machine");
    assert_eq!(
        migrated_note("/x/machines.json.v1-backup"),
        "machines.json was upgraded to the new format. The old file is kept at /x/machines.json.v1-backup."
    );
    assert!(unreadable_note("bad JSON").starts_with("machines.json can't be read: bad JSON."));
    assert_eq!(rename_note(&[]), None);
    assert_eq!(
        rename_note(&["sd-claude".into()]).as_deref(),
        Some("Rename sd-claude to the new name")
    );
    assert_eq!(
        rename_note(&["a".into(), "b".into(), "c".into()]).as_deref(),
        Some("Rename a, b and c to the new names")
    );
}

#[test]
fn the_secret_row_text() {
    let usage = SecretUsage {
        name: "claude-worker".into(),
        machine: "m1".into(),
        remote_path: "~/.config/agents/claude.env".into(),
        format: SecretFormat::KeyValue("CLAUDE_CODE_OAUTH_TOKEN".into()),
    };
    let mut entry = VaultEntry::new("claude-worker", Kind::Token);
    entry.expires = Some("2026-11-18".into());
    let today = expiry::day_number("2026-11-06").unwrap();
    let facts = CheckResult::pass("mode 600, 41 bytes");
    assert_eq!(
        secret_row(&usage, Some(&entry), Some(&facts), today),
        "claude-worker → ~/.config/agents/claude.env · mode 600, 41 bytes · expires in 12 days"
    );
    assert_eq!(
        secret_row(
            &usage,
            Some(&entry),
            Some(&CheckResult::fail("not on the machine")),
            today
        ),
        "claude-worker → ~/.config/agents/claude.env · not on the machine · expires in 12 days"
    );
    assert_eq!(
        secret_row(&usage, None, None, today),
        "claude-worker → ~/.config/agents/claude.env · not in the vault"
    );
    entry.expires = Some("2026-11-01".into());
    assert_eq!(
        secret_row(&usage, Some(&entry), Some(&facts), today),
        "claude-worker → ~/.config/agents/claude.env · mode 600, 41 bytes · expired 2026-11-01"
    );
}

#[test]
fn the_job_form_refuses_unsafe_values_and_keeps_what_it_does_not_edit() {
    let mut d = JobDraft::new("m1");
    assert_eq!(job_problems(&d), vec!["Add at least one repository"]);
    d.repos = vec![JobRepoDraft {
        github: "me/app".into(),
        local_path: "/Users/me/app".into(),
        check_command: "just check".into(),
        ..JobRepoDraft::default()
    }];
    d.kinds = vec!["ios".into()];
    assert!(job_problems(&d).is_empty());
    for bad in ["me", "me/app/x", "me/ap p", "-me/app", "me/app;x", ""] {
        d.repos[0].github = bad.into();
        assert_eq!(
            job_problems(&d),
            vec!["Repository: owner/name, like me/app"],
            "{bad}"
        );
    }
    d.repos[0].github = "me/app".into();
    d.repos[0].machine_path = "~/src/app;x".into();
    assert_eq!(
        job_problems(&d),
        vec!["Machine path: ~/ or an absolute path with plain characters"]
    );
    d.repos[0].machine_path = "~/work/app".into();
    d.repos[0].verify_skill = "../x".into();
    assert_eq!(
        job_problems(&d),
        vec!["Verify skill: a path inside the checkout"]
    );
    d.repos[0].verify_skill = ".claude/skills/verify/SKILL.md".into();
    d.repos[0].local_path = "relative".into();
    assert_eq!(
        job_problems(&d),
        vec!["Checkout on this Mac: an absolute path"]
    );
    d.repos[0].local_path = "/Users/me/app".into();
    d.repos[0].check_command = "  ".into();
    assert_eq!(job_problems(&d), vec!["Check command: required"]);
    d.repos[0].check_command = "just check".into();
    for bad in ["file:///etc/passwd", "https://x/$(id)", "https://"] {
        d.health_url = bad.into();
        assert_eq!(job_problems(&d), vec!["Health URL: http or https"], "{bad}");
    }
    d.health_url = "https://example.com/health".into();
    d.repos.push(d.repos[0].clone());
    assert_eq!(job_problems(&d), vec!["me/app is listed twice"]);
    d.repos.pop();
    assert!(job_problems(&d).is_empty());

    let mut base = SdWorkerSettings::new("m1");
    base.python = "/usr/bin/python3".into();
    base.sd_path = "/home/a/sd.py".into();
    base.kinds = vec!["backend".into()];
    base.token_expires = Some("2027-01-01".into());
    let made = job_settings(&d, Some(&base));
    assert_eq!(made.python, "/usr/bin/python3");
    assert_eq!(made.sd_path, "/home/a/sd.py");
    assert_eq!(made.token_expires.as_deref(), Some("2027-01-01"));
    assert_eq!(made.issues_secret, "m1-github-issues");
    assert_eq!(made.kinds, vec!["ios"]);
    assert_eq!(
        made.health_url.as_deref(),
        Some("https://example.com/health")
    );
    assert_eq!(
        made.repos,
        vec![MachineRepo {
            github: "me/app".into(),
            local_path: "/Users/me/app".into(),
            check_command: "just check".into(),
            machine_path: Some("~/work/app".into()),
            verify_skill: Some(".claude/skills/verify/SKILL.md".into()),
        }]
    );
    d.repos[0].machine_path = String::new();
    d.machine = "m2".into();
    let fresh = job_settings(&d, None);
    assert_eq!(fresh.python, "python3");
    assert_eq!(fresh.sd_path, "");
    assert_eq!(fresh.issues_secret, "m2-github-issues");
    assert_eq!(fresh.repos[0].machine_path, None);
}
