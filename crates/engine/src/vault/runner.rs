//! Runs a command with a secret's variables in its environment (Swift `SecretRunner`), streaming its
//! stdout and stderr through a [`Redactor`] each. One argument runs through `/bin/sh -c`, so the
//! caller can quote it and let that shell expand the variables; more run through `/usr/bin/env`.
//! The command reads this process's stdin. The exit code is the command's, or 128 plus the signal
//! that ended it.

use super::redact::Redactor;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

/// The code when the command couldn't start (as a shell's "command not found").
pub const COULD_NOT_RUN: i32 = 127;

/// What the command is run with.
pub fn command_for(args: &[String]) -> Option<Command> {
    match args {
        [] => None,
        [one] => {
            let mut c = Command::new("/bin/sh");
            c.arg("-c").arg(one);
            Some(c)
        }
        many => {
            let mut c = Command::new("/usr/bin/env");
            c.args(many);
            Some(c)
        }
    }
}

/// Runs `args` with `extra` added to this process's environment, writing its output to `out` and
/// `err` with every value in `hide` replaced by its mask.
pub fn run(
    args: &[String],
    extra: &BTreeMap<String, String>,
    hide: &BTreeMap<String, String>,
    out: &mut (dyn Write + Send),
    err: &mut (dyn Write + Send),
) -> i32 {
    let Some(mut command) = command_for(args) else {
        let _ = writeln!(err, "No command to run.");
        return 2;
    };
    command
        .envs(extra)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            // The command's own text only: the environment (the values) is never in this error.
            let _ = writeln!(err, "Couldn't run {}: {e}", args[0]);
            return COULD_NOT_RUN;
        }
    };
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return COULD_NOT_RUN;
    };
    std::thread::scope(|scope| {
        scope.spawn(move || pump(stdout, hide, out));
        scope.spawn(move || pump(stderr, hide, err));
    });
    match child.wait() {
        Ok(status) => exit_code(status),
        Err(_) => COULD_NOT_RUN,
    }
}

/// Copies one pipe through its own redactor until it closes.
fn pump(mut from: impl Read, hide: &BTreeMap<String, String>, to: &mut (dyn Write + Send)) {
    let mut redactor = Redactor::new(hide);
    let mut buf = vec![0u8; 64 * 1024];
    let mut writable = true;
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let ready = redactor.feed(&buf[..n]);
        // A reader that went away (a closed pipe) still lets the command run to its end.
        if writable && (to.write_all(&ready).is_err() || to.flush().is_err()) {
            writable = false;
        }
    }
    if writable {
        let _ = to.write_all(&redactor.finish());
        let _ = to.flush();
    }
}

#[cfg(unix)]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => code,
        (None, Some(signal)) => 128 + signal,
        (None, None) => COULD_NOT_RUN,
    }
}

#[cfg(not(unix))]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(COULD_NOT_RUN)
}
