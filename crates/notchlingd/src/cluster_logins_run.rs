use notchling_engine::cluster::logins::{argv_read_only, StatusRunner};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

const READ_MAX: u64 = 16 * 1024;
const DRAIN: Duration = Duration::from_millis(500);

pub struct SystemStatus;

#[cfg(unix)]
fn own_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn own_group(_command: &mut Command) {}

#[cfg(unix)]
fn end_group(pid: u32) {
    // SAFETY: killpg only sends a signal; a group that is gone answers ESRCH.
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn end_group(_pid: u32) {}

fn read_into(pipe: impl Read + Send + 'static, tx: Sender<String>) {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.take(READ_MAX).read_to_end(&mut bytes);
        let _ = tx.send(String::from_utf8_lossy(&bytes).into_owned());
    });
}

fn collect(rx: &Receiver<String>, want: usize, within: Duration) -> Vec<String> {
    let deadline = Instant::now() + within;
    let mut got = Vec::new();
    while got.len() < want {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(text) => got.push(text),
            Err(_) => break,
        }
    }
    got
}

impl StatusRunner for SystemStatus {
    fn run(&self, argv: &[String], timeout: Duration) -> Option<(i32, String)> {
        if !argv_read_only(argv) {
            return None;
        }
        let (program, args) = argv.split_first()?;
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        own_group(&mut command);
        let mut child = command.spawn().ok()?;
        let pid = child.id();
        let (tx, rx) = channel();
        let mut want = 0;
        if let Some(pipe) = child.stdout.take() {
            read_into(pipe, tx.clone());
            want += 1;
        }
        if let Some(pipe) = child.stderr.take() {
            read_into(pipe, tx.clone());
            want += 1;
        }
        drop(tx);
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                _ => {
                    end_group(pid);
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        };
        let mut texts = collect(&rx, want, DRAIN);
        if texts.len() < want {
            end_group(pid);
            texts.extend(collect(&rx, want - texts.len(), DRAIN));
        }
        Some((status.code().unwrap_or(-1), texts.join("\n")))
    }
}

#[cfg(test)]
#[path = "cluster_logins_run_tests.rs"]
mod tests;
