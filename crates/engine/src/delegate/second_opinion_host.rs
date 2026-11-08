//! The real [`Host`]: runs the delegate command as a child process (this program's own
//! executable, so Stop is a signal and the Codex and Antigravity engines stay the command's one
//! copy), reads its output and reports it when it ends. Stop is SIGTERM, then SIGKILL eight
//! seconds later if it is still there.

use super::second_opinion_store::{Host, Job};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a stopped command gets before it is killed.
const KILL_AFTER: Duration = Duration::from_secs(8);

pub struct ProcessHost {
    /// The program to run (`notchling`).
    pub program: PathBuf,
    /// Arguments that come before the delegate command's own (none for the real one).
    pub prefix: Vec<String>,
    pub kill_after: Duration,
}

impl ProcessHost {
    pub fn new(program: PathBuf) -> ProcessHost {
        ProcessHost {
            program,
            prefix: Vec::new(),
            kill_after: KILL_AFTER,
        }
    }
}

struct Running {
    child: Arc<Mutex<Child>>,
    kill_after: Duration,
}

fn lock(child: &Mutex<Child>) -> std::sync::MutexGuard<'_, Child> {
    child.lock().unwrap_or_else(|e| e.into_inner())
}

impl Job for Running {
    fn terminate(&self) {
        {
            let mut child = lock(&self.child);
            if !matches!(child.try_wait(), Ok(None)) {
                return;
            }
            #[cfg(unix)]
            {
                if let Ok(pid) = i32::try_from(child.id()) {
                    // SAFETY: `kill` takes a pid and a signal number and touches no memory; the
                    // child has not been reaped (checked above, under the lock that reaps it).
                    unsafe {
                        libc::kill(pid, libc::SIGTERM);
                    }
                }
            }
            #[cfg(not(unix))]
            {
                let _ = child.kill();
            }
        }
        let (child, after) = (self.child.clone(), self.kill_after);
        std::thread::spawn(move || {
            std::thread::sleep(after);
            let mut child = lock(&child);
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
        });
    }
}

impl Host for ProcessHost {
    fn launch(
        &self,
        args: Vec<String>,
        folder: &str,
        done: Box<dyn FnOnce(String) + Send>,
    ) -> Result<Box<dyn Job>, String> {
        let mut child = Command::new(&self.program)
            .args(&self.prefix)
            .args(&args)
            .current_dir(folder)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut stdout = child.stdout.take();
        let child = Arc::new(Mutex::new(child));
        let waiting = child.clone();
        std::thread::spawn(move || {
            let reader = std::thread::spawn(move || {
                let mut data = Vec::new();
                if let Some(out) = stdout.as_mut() {
                    let _ = out.read_to_end(&mut data);
                }
                data
            });
            // Polled, so a stop can take the lock while this waits.
            loop {
                let status = lock(&waiting).try_wait();
                if !matches!(status, Ok(None)) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let data = reader.join().unwrap_or_default();
            done(String::from_utf8_lossy(&data).into_owned());
        });
        Ok(Box::new(Running {
            child,
            kill_after: self.kill_after,
        }))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    /// `/bin/sh -c <script> sh <args…>`: the script stands in for `notchling`.
    fn host(script: &str) -> ProcessHost {
        ProcessHost {
            program: PathBuf::from("/bin/sh"),
            prefix: vec!["-c".into(), script.into(), "sh".into()],
            kill_after: Duration::from_millis(200),
        }
    }

    fn folder() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    #[test]
    fn it_reports_the_output_when_the_command_ends() {
        let (tx, rx) = channel();
        host("echo \"got $1 $2\"; echo '{\"tool\":\"codex\"}'")
            .launch(
                vec!["delegate".into(), "codex".into()],
                &folder(),
                Box::new(move |out| tx.send(out).unwrap()),
            )
            .unwrap();
        let out = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(out, "got delegate codex\n{\"tool\":\"codex\"}\n");
    }

    #[test]
    fn stopping_ends_a_running_command_and_kills_one_that_ignores_it() {
        for script in ["exec sleep 30", "trap '' TERM; exec sleep 30"] {
            let (tx, rx) = channel();
            let job = host(script)
                .launch(
                    vec![],
                    &folder(),
                    Box::new(move |out| tx.send(out).unwrap()),
                )
                .unwrap();
            std::thread::sleep(Duration::from_millis(100));
            job.terminate();
            let out = rx.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(out, "", "{script}");
            // Late, and harmless once it has ended.
            job.terminate();
        }
    }

    #[test]
    fn a_program_that_will_not_start_is_an_error() {
        let host = ProcessHost::new(PathBuf::from("/no/such/notchling"));
        let result = host.launch(vec![], &folder(), Box::new(|_| {}));
        assert!(result.is_err());
    }
}
