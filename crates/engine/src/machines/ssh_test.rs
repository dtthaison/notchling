use super::addresses::{MachineAddress, Route};
use super::runner::{Call, CommandResult, Runner, TIMED_OUT};
use super::shell::ssh_prefix;
use super::ssh_key::{inspect, KeyIssue};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub type Stop = Arc<AtomicBool>;

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
    JumpUntrusted { host: String },
    Unreachable(Why),
    WrongNetwork { wanted: String, active: String },
    Key(KeyIssue),
}

impl SshTest {
    pub fn passed(&self) -> bool {
        matches!(self, SshTest::Connected { .. })
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
        "-o",
        "ForwardAgent=no",
        "-o",
        "ForwardX11=no",
        "-o",
        "PermitLocalCommand=no",
        "-o",
        "ClearAllForwardings=yes",
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

fn named_host(text: &str) -> Option<String> {
    if !text.contains("Host key verification failed") {
        return None;
    }
    text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("No ")?;
        let (_, after) = rest.split_once(" host key is known for ")?;
        let word = after.split_whitespace().next()?;
        let bare = match word.strip_prefix('[') {
            Some(inner) => inner.split(']').next().unwrap_or(inner),
            None => word,
        };
        Some(bare.to_owned())
    })
}

fn is_jump_host(route: &Route, host: &str) -> bool {
    let mut at = route.jump.as_deref();
    while let Some(hop) = at {
        if hop.host == host && route.host != host {
            return true;
        }
        at = hop.jump.as_deref();
    }
    false
}

fn host_key_changed(text: &str) -> bool {
    [
        "HOST IDENTIFICATION HAS CHANGED",
        "REVOKED HOST KEY",
        "POSSIBLE DNS SPOOFING",
    ]
    .iter()
    .any(|w| text.contains(w))
}

fn failure(result: &CommandResult, route: &Route) -> SshTest {
    let text = format!("{}\n{}", result.err, result.out);
    let has = |w: &str| text.contains(w);
    if host_key_changed(&text) {
        return SshTest::HostKeyChanged;
    }
    if let Some(host) = named_host(&text).filter(|h| is_jump_host(route, h)) {
        return SshTest::JumpUntrusted { host };
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

fn once(
    runner: &dyn Runner,
    route: &Route,
    trust: Trust,
    stop: Option<&Stop>,
) -> Result<String, (SshTest, bool)> {
    let mut call = Call::secs(test_argv(route, trust), TEST_SECS);
    if let Some(stop) = stop {
        call = call.cancellable(stop.clone());
    }
    let result = runner.run(&call);
    if result.code == 0 {
        return Ok(remote_user(&result.out, route));
    }
    let text = format!("{}\n{}", result.err, result.out);
    let unknown =
        !host_key_changed(&text) && named_host(&text).is_some_and(|h| !is_jump_host(route, &h));
    Err((failure(&result, route), unknown))
}

pub fn run(runner: &dyn Runner, route: &Route, home: &Path) -> SshTest {
    run_stoppable(runner, route, home, None)
}

pub fn run_stoppable(
    runner: &dyn Runner,
    route: &Route,
    home: &Path,
    stop: Option<&Stop>,
) -> SshTest {
    if let Some(key) = &route.key {
        if let Some(issue) = inspect(key, home).issue {
            return SshTest::Key(issue);
        }
    }
    match once(runner, route, Trust::Known, stop) {
        Ok(user) => SshTest::Connected { user },
        Err((_, true)) => match once(runner, route, Trust::Unsaved, stop) {
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
    run_target_stoppable(runner, target, home, None)
}

pub fn run_target_stoppable(
    runner: &dyn Runner,
    target: &Target,
    home: &Path,
    stop: Option<&Stop>,
) -> SshTest {
    match target {
        Target::Route(route) => run_stoppable(runner, route, home, stop),
        Target::WrongNetwork { wanted, active } => SshTest::WrongNetwork {
            wanted: wanted.clone(),
            active: active.clone(),
        },
        Target::Unusable => SshTest::Unreachable(Why::Other),
    }
}
