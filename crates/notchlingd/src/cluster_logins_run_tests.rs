use super::*;

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_runner_refuses_a_command_that_could_write_a_login() {
    for parts in [
        vec!["/bin/sh", "-c", "codex login"],
        vec!["codex", "login", "--device-auth"],
        vec!["codex", "refresh"],
        vec!["/bin/echo", "logout"],
    ] {
        assert_eq!(
            SystemStatus.run(&argv(&parts), Duration::from_secs(1)),
            None
        );
    }
}

#[cfg(unix)]
fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(unix)]
fn pid_from(path: &std::path::Path) -> i32 {
    for _ in 0..100 {
        if let Some(pid) = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| t.trim().parse().ok())
        {
            return pid;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("no pid");
}

#[cfg(unix)]
fn dies(pid: i32) -> bool {
    for _ in 0..200 {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

#[cfg(unix)]
fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-lr-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(unix)]
#[test]
fn the_runner_reads_output_and_the_exit_code_and_kills_a_slow_command() {
    let said = argv(&[
        "/bin/sh",
        "-c",
        "echo Not logged in; echo oops 1>&2; exit 3",
    ]);
    let (code, text) = SystemStatus.run(&said, Duration::from_secs(5)).unwrap();
    assert_eq!(code, 3);
    assert!(
        text.contains("Not logged in") && text.contains("oops"),
        "{text}"
    );
    let started = Instant::now();
    let slow = argv(&["/bin/sleep", "30"]);
    assert_eq!(SystemStatus.run(&slow, Duration::from_millis(150)), None);
    assert!(started.elapsed() < Duration::from_secs(5));
    let gone = argv(&["/nonexistent/codex", "login", "status"]);
    assert_eq!(SystemStatus.run(&gone, Duration::from_secs(1)), None);
}

#[cfg(unix)]
#[test]
fn a_grandchild_holding_the_pipes_does_not_hold_the_probe_and_is_killed() {
    let dir = scratch("grand");
    let pidfile = dir.join("pid");
    let script = format!(
        "sleep 30 & echo $! > '{}'; echo Logged in; exit 0",
        pidfile.display()
    );
    let started = Instant::now();
    let got = SystemStatus.run(&argv(&["/bin/sh", "-c", &script]), Duration::from_secs(5));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    let (code, text) = got.unwrap();
    assert_eq!(code, 0);
    assert!(text.contains("Logged in"), "{text}");
    assert!(dies(pid_from(&pidfile)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_timeout_kills_the_whole_group_and_returns_unknown_quickly() {
    let dir = scratch("timeout");
    let pidfile = dir.join("pid");
    let script = format!("sleep 30 & echo $! > '{}'; wait", pidfile.display());
    let started = Instant::now();
    let got = SystemStatus.run(
        &argv(&["/bin/sh", "-c", &script]),
        Duration::from_millis(400),
    );
    assert_eq!(got, None);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    assert!(dies(pid_from(&pidfile)));
    let _ = std::fs::remove_dir_all(&dir);
}
