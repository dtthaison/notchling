//! `notchling-ptyhost`: the session host (`legacy/macos/Sources/hostd/main.swift`).
//!
//!   notchling-ptyhost                          run the host
//!   notchling-ptyhost --status                 print {"running":N,"total":M} and exit (asks the running host)
//!   notchling-ptyhost --record-resume SESSION_ID CWD    remember a session for the next start
//!   notchling-ptyhost --socket PATH --state PATH        (tests: other paths)
//!   notchling-ptyhost --version

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("{}", notchling_ptyhost::version_line());
        return;
    }
    std::process::exit(run(&args));
}

#[cfg(not(unix))]
fn run(_args: &[String]) -> i32 {
    println!("hostd: the session host runs on macOS and Linux only");
    1
}

#[cfg(unix)]
fn run(args: &[String]) -> i32 {
    unix::run(args)
}

#[cfg(unix)]
mod unix {
    use notchling_ptyhost::client::HostClient;
    use notchling_ptyhost::host::{record_for_resume, HostOptions, SessionHost};
    use notchling_ptyhost::pty_handle::PtySpawner;
    use notchling_ptyhost::server::{same_user, HostServer};
    use notchling_ptyhost::state::{decide, StateStore, UpdateAction};
    use notchling_ptyhost::{support_dir, system};
    use serde_json::Map;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    static STOP: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_sigterm(_: libc::c_int) {
        // Only an atomic store: async-signal-safe.
        STOP.store(true, Ordering::SeqCst);
    }

    fn argument(args: &[String], name: &str) -> Option<String> {
        let index = args.iter().position(|a| a == name)?;
        args.get(index + 1).cloned()
    }

    fn path_or_default(args: &[String], flag: &str, name: &str) -> Option<PathBuf> {
        argument(args, flag)
            .map(PathBuf::from)
            .or_else(|| support_dir().map(|d| d.join(name)))
    }

    pub fn run(args: &[String]) -> i32 {
        let (Some(socket), Some(state_path)) = (
            path_or_default(args, "--socket", "hostd.sock"),
            path_or_default(args, "--state", "hostd-state.json"),
        ) else {
            println!("hostd: no home folder to keep the socket in");
            return 1;
        };

        if args.iter().any(|a| a == "--status") {
            return status(&socket);
        }

        // Migration: remember a session that is running somewhere else right now, so the next host
        // start resumes it.
        if let Some(index) = args.iter().position(|a| a == "--record-resume") {
            let (Some(session), Some(cwd)) = (args.get(index + 1), args.get(index + 2)) else {
                println!("hostd: --record-resume SESSION_ID CWD");
                return 1;
            };
            record_for_resume(&StateStore::new(&state_path), session, cwd, None);
            println!("recorded {session} for resume");
            return 0;
        }

        serve(&socket, &state_path)
    }

    fn status(socket: &Path) -> i32 {
        let answer = HostClient::connect(socket, Duration::from_secs(2))
            .and_then(|client| client.request("status", Map::new(), Duration::from_secs(2)));
        match answer {
            Ok(reply) => {
                let number = |key: &str| reply.get(key).and_then(|v| v.as_i64()).unwrap_or(0);
                println!(
                    "{{\"running\":{},\"total\":{}}}",
                    number("running"),
                    number("total")
                );
            }
            // No host answering: nothing is running.
            Err(_) => println!("{{\"running\":0,\"total\":0,\"host\":false}}"),
        }
        0
    }

    fn serve(socket: &Path, state_path: &Path) -> i32 {
        let state = Arc::new(StateStore::new(state_path));
        let sessions_dir = system::home().map(|h| h.join(".claude/sessions"));
        let options = HostOptions {
            discover_session_id: Arc::new(move |pid| {
                let dir = sessions_dir.as_ref()?;
                system::session_id_for_root_pid(pid, dir, &system::parent_pid)
            }),
            ..HostOptions::default()
        };
        let host = SessionHost::new(Arc::new(PtySpawner::new()), state, options);
        let server = HostServer::new(host.clone(), socket, same_user());
        if let Err(error) = server.start() {
            println!("hostd: cannot start: {error}");
            return 1;
        }
        println!("hostd: listening on {}", socket.display());

        // Bring back sessions that were recorded and whose process is gone (a reboot, a crash, an update).
        let resumed = host.resume_recorded();
        if resumed > 0 {
            println!("hostd: resumed {resumed} session(s)");
        }

        // SIGTERM from launchd: stop listening and leave. The ptys end with the host, and the next
        // start resumes them.
        let handler = on_sigterm as extern "C" fn(libc::c_int) as libc::sighandler_t;
        // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
        unsafe { libc::signal(libc::SIGTERM, handler) };

        // An update was installed next to the running binary: take it over as soon as nothing runs.
        let binary = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .unwrap_or_default();
        let started = stamp(&binary);
        let mut next_check = Instant::now() + Duration::from_secs(30);
        loop {
            std::thread::sleep(Duration::from_millis(200));
            if STOP.load(Ordering::SeqCst) {
                server.stop();
                return 0;
            }
            if Instant::now() < next_check {
                continue;
            }
            next_check = Instant::now() + Duration::from_secs(30);
            if decide(stamp(&binary) != started, host.running_count()) == UpdateAction::RestartNow {
                println!("hostd: a newer helper is installed and nothing is running; restarting");
                server.stop();
                return 0; // launchd KeepAlive starts the new binary
            }
        }
    }

    /// The binary's modification time and size.
    fn stamp(binary: &Path) -> String {
        let Ok(meta) = std::fs::metadata(binary) else {
            return "0-0".to_string();
        };
        let modified = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        format!("{modified}-{}", meta.len())
    }
}
