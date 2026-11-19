use super::addresses::Route;
use super::fakes::TempDir;
use super::runner::{CommandResult, Recorded, ScriptedRunner, TIMED_OUT};
use super::ssh_key::KeyIssue;
use super::ssh_test::*;
use std::path::Path;

fn route() -> Route {
    Route::direct("app-1.tail1234.ts.net", "ubuntu")
}

fn is_strict(r: &Recorded) -> bool {
    r.argv.iter().any(|a| a == "StrictHostKeyChecking=yes")
}

fn is_loose(r: &Recorded) -> bool {
    r.argv.iter().any(|a| a == "StrictHostKeyChecking=no")
}

#[test]
fn the_test_asks_who_it_is_with_no_prompts_no_mux_and_a_strict_known_hosts() {
    let argv = test_argv(&route(), Trust::Known);
    assert_eq!(argv[0], "/usr/bin/ssh");
    for want in [
        "BatchMode=yes",
        "NumberOfPasswordPrompts=0",
        "ControlPath=none",
        "UpdateHostKeys=no",
        "StrictHostKeyChecking=yes",
        "ConnectTimeout=5",
    ] {
        assert!(argv.contains(&want.to_owned()), "{want}: {argv:?}");
    }
    let dashes = argv.iter().position(|a| a == "--").unwrap();
    assert_eq!(argv[dashes + 1], "ubuntu@app-1.tail1234.ts.net");
    assert_eq!(&argv[dashes + 2..], ["id", "-un"]);
    assert!(argv
        .iter()
        .take(dashes)
        .all(|a| !a.contains("known_hosts") && a != "StrictHostKeyChecking=no"));
}

#[test]
fn the_unsaved_pass_never_reads_or_writes_a_known_hosts_file() {
    let argv = test_argv(&route(), Trust::Unsaved);
    for want in [
        "StrictHostKeyChecking=no",
        "UserKnownHostsFile=/dev/null",
        "GlobalKnownHostsFile=/dev/null",
        "UpdateHostKeys=no",
    ] {
        assert!(argv.contains(&want.to_owned()), "{want}: {argv:?}");
    }
}

#[test]
fn the_key_and_the_port_reach_the_test() {
    let r = Route {
        port: Some(2222),
        ..route().with_key(Some("~/.ssh/id_work"))
    };
    let argv = test_argv(&r, Trust::Known);
    let at = argv.iter().position(|a| a == "-i").unwrap();
    assert_eq!(
        &argv[at..at + 4],
        ["-i", "~/.ssh/id_work", "-o", "IdentitiesOnly=yes"]
    );
    assert!(argv.windows(2).any(|w| w == ["-p", "2222"]));
}

fn run_with(runner: &ScriptedRunner, r: &Route) -> SshTest {
    run(runner, r, Path::new("/nonexistent-home"))
}

#[test]
fn it_says_who_it_connected_as() {
    let runner = ScriptedRunner::new();
    runner.on(|_| true, CommandResult::ok("ubuntu\n"));
    assert_eq!(
        run_with(&runner, &route()),
        SshTest::Connected {
            user: "ubuntu".into()
        }
    );
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn a_refused_login_is_a_wrong_user_or_key() {
    let runner = ScriptedRunner::new();
    runner.on(
        |_| true,
        CommandResult::new(255, "", "ubuntu@app-1: Permission denied (publickey)."),
    );
    assert_eq!(run_with(&runner, &route()), SshTest::Denied);
}

#[test]
fn a_changed_host_key_is_named_and_never_retried_loosely() {
    let runner = ScriptedRunner::new();
    runner.on(
        |_| true,
        CommandResult::new(
            255,
            "",
            "@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@\nHost key verification failed.",
        ),
    );
    assert_eq!(run_with(&runner, &route()), SshTest::HostKeyChanged);
    assert_eq!(runner.calls().len(), 1);
    assert!(is_strict(&runner.calls()[0]));
}

#[test]
fn a_new_host_is_tested_once_more_without_saving_anything() {
    let runner = ScriptedRunner::new();
    runner.on(is_loose_args, CommandResult::ok("ubuntu\n"));
    runner.on(
        |_| true,
        CommandResult::new(
            255,
            "",
            "No ED25519 host key is known for app-1 and you have requested strict checking.\nHost key verification failed.",
        ),
    );
    assert_eq!(
        run_with(&runner, &route()),
        SshTest::NewHost {
            user: "ubuntu".into()
        }
    );
    let calls = runner.calls();
    assert_eq!(calls.len(), 2);
    assert!(is_strict(&calls[0]) && is_loose(&calls[1]));
    assert!(calls[1]
        .argv
        .contains(&"UserKnownHostsFile=/dev/null".to_owned()));
}

fn is_loose_args(a: &[String]) -> bool {
    a.iter().any(|x| x == "StrictHostKeyChecking=no")
}

#[test]
fn a_new_host_with_a_wrong_user_still_says_so() {
    let runner = ScriptedRunner::new();
    runner.on(
        is_loose_args,
        CommandResult::new(255, "", "Permission denied (publickey)."),
    );
    runner.on(
        |_| true,
        CommandResult::new(255, "", "Host key verification failed."),
    );
    assert_eq!(run_with(&runner, &route()), SshTest::Denied);
}

#[test]
fn what_cannot_be_reached_says_why() {
    let cases = [
        (TIMED_OUT, "", Why::TimedOut),
        (
            255,
            "ssh: connect to host x port 22: Operation timed out",
            Why::TimedOut,
        ),
        (
            255,
            "ssh: connect to host x port 22: Connection refused",
            Why::Refused,
        ),
        (
            255,
            "ssh: Could not resolve hostname x: nodename nor servname",
            Why::NoName,
        ),
        (
            255,
            "ssh: connect to host x port 22: No route to host",
            Why::NoRoute,
        ),
        (
            255,
            "ssh: connect to host x port 22: Network is unreachable",
            Why::NoRoute,
        ),
        (
            255,
            "kex_exchange_identification: read: Connection reset",
            Why::Other,
        ),
    ];
    for (code, err, why) in cases {
        let runner = ScriptedRunner::new();
        runner.on(|_| true, CommandResult::new(code, "", err));
        assert_eq!(
            run_with(&runner, &route()),
            SshTest::Unreachable(why),
            "{err}"
        );
    }
}

#[test]
fn a_bad_key_file_stops_before_ssh_runs() {
    let dir = TempDir::new("sshtest");
    let runner = ScriptedRunner::new();
    let missing = route().with_key(Some("~/.ssh/nope"));
    assert_eq!(
        run(&runner, &missing, dir.path()),
        SshTest::Key(KeyIssue::Missing)
    );
    assert!(runner.calls().is_empty());
}

#[cfg(unix)]
#[test]
fn a_good_key_file_lets_the_test_run_with_it() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("sshtest");
    let key = dir.join("id_work");
    std::fs::write(&key, "fixture").unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let runner = ScriptedRunner::new();
    runner.on(|_| true, CommandResult::ok("ubuntu\n"));
    let r = route().with_key(Some("~/id_work"));
    assert!(run(&runner, &r, dir.path()).passed());
    assert!(runner.calls()[0].argv.contains(&"~/id_work".to_owned()));
}

#[test]
fn a_wrong_network_never_runs_ssh() {
    let runner = ScriptedRunner::new();
    let target = Target::WrongNetwork {
        wanted: "work.example".into(),
        active: "person@example.com".into(),
    };
    let result = run_target(&runner, &target, Path::new("/x"));
    assert_eq!(
        result,
        SshTest::WrongNetwork {
            wanted: "work.example".into(),
            active: "person@example.com".into()
        }
    );
    assert!(runner.calls().is_empty());
    assert!(!result.passed());
}
