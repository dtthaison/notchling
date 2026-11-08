//! The real [`Runner`] (Swift `ProcessRunner`): a command in a process group of its own, both pipes
//! read on threads so a full pipe never stops it, stdin written on a thread of its own. A timeout
//! signals the whole group (SIGTERM, then SIGKILL after two seconds), and what the command left
//! running is ended when it exits. Each pipe keeps its last 2 MiB; a cut start says so with
//! [`CUT_NOTICE`].

use super::runner::{Call, Channel, CommandResult, Runner, NOT_FOUND, TIMED_OUT};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The most output kept from one pipe; what came before is cut.
pub const OUTPUT_CAP: usize = 2 * 1024 * 1024;
/// The first line of a pipe's text when its start was cut.
pub const CUT_NOTICE: &str = "[earlier output cut]\n";
/// The code when the exit status could not be read.
pub const LOST_STATUS: i32 = 125;
const TERM_GRACE: Duration = Duration::from_secs(2);
const DRAIN_GRACE: Duration = Duration::from_secs(3);

/// Runs real processes.
pub struct ProcessRunner;

#[derive(Default)]
struct Sink {
    data: Vec<u8>,
    cut: bool,
}

impl Sink {
    fn append(&mut self, chunk: &[u8]) {
        self.data.extend_from_slice(chunk);
        if self.data.len() > OUTPUT_CAP + 256 * 1024 {
            let drop = self.data.len() - OUTPUT_CAP;
            self.data.drain(..drop);
            self.cut = true;
        }
    }

    fn text(&self) -> String {
        let mut tail: &[u8] = &self.data;
        let mut cut = self.cut;
        if tail.len() > OUTPUT_CAP {
            tail = &tail[tail.len() - OUTPUT_CAP..];
            cut = true;
        }
        if !cut {
            return String::from_utf8_lossy(tail).into_owned();
        }
        // the cut may fall inside a character: start at the next one
        let start = tail
            .iter()
            .position(|b| b & 0xC0 != 0x80)
            .unwrap_or(tail.len());
        format!("{CUT_NOTICE}{}", String::from_utf8_lossy(&tail[start..]))
    }
}

/// How many bytes at the end of `bytes` begin a UTF-8 character that isn't complete yet.
pub fn incomplete_tail(bytes: &[u8]) -> usize {
    let mut back = 0;
    for &byte in bytes.iter().rev().take(3) {
        back += 1;
        if byte & 0xC0 == 0x80 {
            continue;
        }
        let need = if byte >= 0xF0 {
            4
        } else if byte >= 0xE0 {
            3
        } else if byte >= 0xC0 {
            2
        } else {
            1
        };
        return if need > back { back } else { 0 };
    }
    0
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn fail(message: String) -> CommandResult {
    CommandResult {
        code: NOT_FOUND,
        out: String::new(),
        err: message,
    }
}

/// Signals the group: SIGTERM, then SIGKILL after the grace.
#[cfg(unix)]
fn end_group(pgid: u32) {
    let pgid = pgid as libc::pid_t;
    // SAFETY: killpg only sends a signal; a group that is gone answers ESRCH.
    unsafe {
        if libc::killpg(pgid, 0) != 0 {
            return;
        }
        libc::killpg(pgid, libc::SIGTERM);
    }
    let deadline = Instant::now() + TERM_GRACE;
    // SAFETY: as above.
    while Instant::now() < deadline && unsafe { libc::killpg(pgid, 0) } == 0 {
        std::thread::sleep(Duration::from_millis(50));
    }
    // SAFETY: as above.
    unsafe {
        if libc::killpg(pgid, 0) == 0 {
            libc::killpg(pgid, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn end_group(_pgid: u32) {}

impl Runner for ProcessRunner {
    fn run(&self, call: &Call) -> CommandResult {
        let Some(exe) = call.argv.first() else {
            return fail("no command".into());
        };
        if !std::path::Path::new(exe).is_file() {
            return fail(format!("{exe} not found"));
        }
        let mut command = Command::new(exe);
        command.args(&call.argv[1..]);
        for (k, v) in &call.env {
            command.env(k, v);
        }
        if let Some(dir) = &call.cwd {
            command.current_dir(dir);
        }
        command
            .stdin(if call.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => return fail(e.to_string()),
        };
        let pid = child.id();
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if let (Some(data), Some(mut pipe)) = (call.stdin.clone(), child.stdin.take()) {
            std::thread::spawn(move || {
                // a child that never reads (or has gone) ends the write with EPIPE
                let _ = pipe.write_all(&data);
            });
        }
        let mut readers = Vec::new();
        let mut sinks = Vec::new();
        let pipes: Vec<(Channel, Box<dyn Read + Send>)> = [
            child
                .stdout
                .take()
                .map(|p| (Channel::Stdout, Box::new(p) as Box<dyn Read + Send>)),
            child
                .stderr
                .take()
                .map(|p| (Channel::Stderr, Box::new(p) as Box<dyn Read + Send>)),
        ]
        .into_iter()
        .flatten()
        .collect();
        for (channel, mut pipe) in pipes {
            let sink = Arc::new(Mutex::new(Sink::default()));
            sinks.push((channel, sink.clone()));
            let stream = call.on_output.clone();
            let done = finished.clone();
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || {
                let mut buffer = vec![0u8; 65536];
                let mut held: Vec<u8> = Vec::new();
                let send = |bytes: &[u8]| {
                    if let Some(s) = &stream {
                        if !bytes.is_empty() && !done.load(std::sync::atomic::Ordering::SeqCst) {
                            s(channel, &String::from_utf8_lossy(bytes));
                        }
                    }
                };
                loop {
                    match pipe.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            lock(&sink).append(&buffer[..n]);
                            held.extend_from_slice(&buffer[..n]);
                            let keep = incomplete_tail(&held);
                            let ready = held.len() - keep;
                            send(&held[..ready]);
                            held.drain(..ready);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                send(&held);
                let _ = tx.send(());
            });
            readers.push(rx);
        }

        let deadline = Instant::now() + call.timeout;
        let mut timed_out = false;
        let mut status = None;
        loop {
            match child.try_wait() {
                Ok(Some(s)) => {
                    status = Some(s);
                    break;
                }
                Ok(None) if Instant::now() >= deadline => {
                    timed_out = true;
                    end_group(pid);
                    let _ = child.kill();
                    status = child.wait().ok();
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => break,
            }
        }
        // what the command left running must not outlive it
        end_group(pid);
        let drain = Instant::now() + DRAIN_GRACE;
        for rx in readers {
            let left = drain.saturating_duration_since(Instant::now());
            let _ = rx.recv_timeout(left);
        }
        finished.store(true, std::sync::atomic::Ordering::SeqCst);
        let text = |c: Channel| {
            sinks
                .iter()
                .find(|(ch, _)| *ch == c)
                .map(|(_, s)| lock(s).text())
                .unwrap_or_default()
        };
        let code = if timed_out {
            TIMED_OUT
        } else {
            match status {
                Some(s) => exit_code(s),
                None => super::process::LOST_STATUS,
            }
        };
        CommandResult {
            code,
            out: text(Channel::Stdout),
            err: text(Channel::Stderr),
        }
    }
}

/// A normal exit gives its code; death by a signal gives the signal number.
fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return signal;
        }
    }
    LOST_STATUS
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::machines::runner::argv;

    #[test]
    fn utf8_tails_wait_for_the_rest_of_their_character() {
        assert_eq!(incomplete_tail(b"abc"), 0);
        assert_eq!(incomplete_tail(&[b'a', 0xE2, 0x82]), 2);
        assert_eq!(incomplete_tail(&[0xE2, 0x82, 0xAC]), 0);
        assert_eq!(incomplete_tail(&[0xF0]), 1);
    }

    #[test]
    fn a_long_output_keeps_its_end_and_says_it_was_cut() {
        let mut sink = Sink::default();
        sink.append(&vec![b'a'; OUTPUT_CAP + 300 * 1024]);
        sink.append(b"END");
        let text = sink.text();
        assert!(text.starts_with(CUT_NOTICE));
        assert!(text.ends_with("END"));
    }

    #[test]
    fn a_missing_executable_is_127_and_nothing_runs() {
        let r = ProcessRunner.run(&Call::secs(argv(&["/nonexistent/tool-for-tests"]), 5));
        assert_eq!(r.code, NOT_FOUND);
        assert!(r.err.contains("not found"));
    }

    #[test]
    fn stdin_reaches_the_command_and_output_streams_with_its_pipe() {
        let seen = Arc::new(Mutex::new(Vec::<(Channel, String)>::new()));
        let s = seen.clone();
        let call = Call::secs(argv(&["/bin/sh", "-c", "cat; echo oops >&2; exit 3"]), 10)
            .stdin(b"hello".to_vec())
            .streaming(Some(Arc::new(move |c, t: &str| {
                lock(&s).push((c, t.to_string()))
            })));
        let r = ProcessRunner.run(&call);
        assert_eq!(r.code, 3);
        assert_eq!(r.out, "hello");
        assert_eq!(r.err, "oops\n");
        let seen = lock(&seen).clone();
        assert!(seen.contains(&(Channel::Stdout, "hello".into())));
        assert!(seen
            .iter()
            .any(|(c, t)| *c == Channel::Stderr && t.contains("oops")));
    }

    #[test]
    fn a_timeout_ends_the_whole_group_with_124() {
        let started = Instant::now();
        let r = ProcessRunner.run(&Call::new(
            argv(&["/bin/sh", "-c", "sleep 30 & sleep 30"]),
            Duration::from_millis(300),
        ));
        assert_eq!(r.code, TIMED_OUT);
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
