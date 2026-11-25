use super::redact::Redactor;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

pub const COULD_NOT_RUN: i32 = 127;

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
