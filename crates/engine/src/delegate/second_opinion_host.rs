use super::second_opinion_store::{Host, Job};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const KILL_AFTER: Duration = Duration::from_secs(8);

pub struct ProcessHost {
    pub program: PathBuf,
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

fn ask_to_end(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: `kill` takes a pid and a signal number and touches no memory; the child has
            // not been reaped (the caller checked, under the lock that reaps it).
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

impl Job for Running {
    fn terminate(&self) {
        {
            let mut child = lock(&self.child);
            if !matches!(child.try_wait(), Ok(None)) {
                return;
            }
            ask_to_end(&mut child);
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

    fn stop_within(&self, grace: Duration) {
        let deadline = Instant::now() + grace;
        let mut asked = false;
        loop {
            {
                let mut child = lock(&self.child);
                if !matches!(child.try_wait(), Ok(None)) {
                    return;
                }
                if !asked {
                    ask_to_end(&mut child);
                    asked = true;
                } else if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
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
            job.terminate();
        }
    }

    fn alive(pid: i32) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[test]
    fn stop_within_returns_only_once_the_command_is_gone() {
        for (trap, kill_needed) in [("", false), ("trap '' TERM; ", true)] {
            let pid_file = std::env::temp_dir().join(format!(
                "nl-stop-within-{}-{}",
                std::process::id(),
                kill_needed
            ));
            let script = format!("{trap}echo $$ > '{}'; exec sleep 30", pid_file.display());
            let (tx, rx) = channel();
            let mut slow = host(&script);
            slow.kill_after = Duration::from_secs(60);
            let job = slow
                .launch(
                    vec![],
                    &folder(),
                    Box::new(move |out| tx.send(out).unwrap()),
                )
                .unwrap();
            let pid = loop {
                if let Some(pid) = std::fs::read_to_string(&pid_file)
                    .ok()
                    .and_then(|t| t.trim().parse::<i32>().ok())
                {
                    break pid;
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            let started = std::time::Instant::now();
            job.stop_within(Duration::from_millis(400));
            let took = started.elapsed();
            assert!(!alive(pid), "{script}: it is gone when stop_within returns");
            if kill_needed {
                assert!(took >= Duration::from_millis(400), "{script}: {took:?}");
            }
            assert!(took < Duration::from_secs(3), "{script}: {took:?}");
            assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), "");
            job.stop_within(Duration::from_millis(400));
            let _ = std::fs::remove_file(&pid_file);
        }
    }

    #[test]
    fn a_program_that_will_not_start_is_an_error() {
        let host = ProcessHost::new(PathBuf::from("/no/such/notchling"));
        let result = host.launch(vec![], &folder(), Box::new(|_| {}));
        assert!(result.is_err());
    }
}
