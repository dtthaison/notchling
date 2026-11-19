use super::addresses::{MachineAddress, Route};
use super::runner::{Call, CommandResult, Runner, TIMED_OUT};
use super::shell::ssh_prefix;
use super::ssh_key::{inspect, KeyIssue};
use std::path::Path;

pub const TEST_CONNECT_SECS: u64 = 5;
pub const TEST_SECS: u64 = 12;
pub const TEST_PARALLEL: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    TimedOut,
    Refused,
    NoName,
    NoRoute,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SshTest {
    Connected { user: String },
    NewHost { user: String },
    Denied,
    HostKeyChanged,
    Unreachable(Why),
    WrongNetwork { wanted: String, active: String },
    Key(KeyIssue),
}

impl SshTest {
    pub fn passed(&self) -> bool {
        matches!(self, SshTest::Connected { .. } | SshTest::NewHost { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    Known,
    Unsaved,
}

pub fn test_argv(route: &Route, trust: Trust) -> Vec<String> {
    let mut argv = ssh_prefix(route, TEST_CONNECT_SECS);
    let at = argv.iter().position(|a| a == "--").unwrap_or(argv.len());
    let mut extra: Vec<&str> = vec![
        "-o",
        "NumberOfPasswordPrompts=0",
        "-o",
        "ControlPath=none",
        "-o",
        "UpdateHostKeys=no",
    ];
    match trust {
        Trust::Known => extra.extend(["-o", "StrictHostKeyChecking=yes"]),
        Trust::Unsaved => extra.extend([
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "GlobalKnownHostsFile=/dev/null",
        ]),
    }
    let tail = argv.split_off(at);
    argv.extend(extra.into_iter().map(str::to_owned));
    argv.extend(tail);
    argv.extend(["id".to_owned(), "-un".to_owned()]);
    argv
}

fn remote_user(out: &str, route: &Route) -> String {
    out.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && l.len() <= 64 && !l.chars().any(char::is_whitespace))
        .unwrap_or(route.user.as_str())
        .to_owned()
}

fn failure(result: &CommandResult) -> SshTest {
    let text = format!("{}\n{}", result.err, result.out);
    let has = |w: &str| text.contains(w);
    if has("HOST IDENTIFICATION HAS CHANGED") {
        return SshTest::HostKeyChanged;
    }
    if has("Permission denied") || has("Too many authentication failures") {
        return SshTest::Denied;
    }
    SshTest::Unreachable(if result.code == TIMED_OUT || has("timed out") {
        Why::TimedOut
    } else if has("Connection refused") {
        Why::Refused
    } else if has("Could not resolve hostname") {
        Why::NoName
    } else if has("No route to host") || has("Network is unreachable") {
        Why::NoRoute
    } else {
        Why::Other
    })
}

fn once(runner: &dyn Runner, route: &Route, trust: Trust) -> Result<String, (SshTest, bool)> {
    let result = runner.run(&Call::secs(test_argv(route, trust), TEST_SECS));
    if result.code == 0 {
        return Ok(remote_user(&result.out, route));
    }
    let text = format!("{}\n{}", result.err, result.out);
    let unknown = !text.contains("HOST IDENTIFICATION HAS CHANGED")
        && text.contains("Host key verification failed");
    Err((failure(&result), unknown))
}

pub fn run(runner: &dyn Runner, route: &Route, home: &Path) -> SshTest {
    if let Some(key) = &route.key {
        if let Some(issue) = inspect(key, home).issue {
            return SshTest::Key(issue);
        }
    }
    match once(runner, route, Trust::Known) {
        Ok(user) => SshTest::Connected { user },
        Err((_, true)) => match once(runner, route, Trust::Unsaved) {
            Ok(user) => SshTest::NewHost { user },
            Err((other, _)) => other,
        },
        Err((other, false)) => other,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Route(Route),
    WrongNetwork { wanted: String, active: String },
    Unusable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tested {
    pub address: MachineAddress,
    pub result: SshTest,
}

pub fn run_target(runner: &dyn Runner, target: &Target, home: &Path) -> SshTest {
    match target {
        Target::Route(route) => run(runner, route, home),
        Target::WrongNetwork { wanted, active } => SshTest::WrongNetwork {
            wanted: wanted.clone(),
            active: active.clone(),
        },
        Target::Unusable => SshTest::Unreachable(Why::Other),
    }
}
