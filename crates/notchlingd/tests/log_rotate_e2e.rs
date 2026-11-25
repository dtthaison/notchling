#![cfg(unix)]

use notchling_local_api::{ensure_daemon_with, Client, ClientKind, SpawnOptions, Started};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn stop(socket: &Path) {
    let Ok(mut client) = Client::connect(socket, ClientKind::Cli, "0.1.0") else {
        return;
    };
    let Ok(status) = client.status() else { return };
    let pid = status.pid as libc::pid_t;
    unsafe { libc::kill(pid, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(10);
    while unsafe { libc::kill(pid, 0) } == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    if unsafe { libc::kill(pid, 0) } == 0 {
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
}

#[test]
fn a_daemon_that_starts_over_a_log_past_five_megabytes_moves_it_aside() {
    let dir = Dir(PathBuf::from(format!(
        "/tmp/nl-logrot-e2e-{}",
        std::process::id()
    )));
    let _ = std::fs::remove_dir_all(&dir.0);
    let root = dir.0.join("root");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/fixtures/notch"),
        &root,
    );
    let state = dir.0.join("s");
    std::fs::create_dir_all(&state).unwrap();
    let log = state.join("notchlingd.log");
    let big = 5 * 1024 * 1024 + 1;
    std::fs::write(&log, vec![b'x'; big]).unwrap();
    let socket = state.join("notchlingd.sock");
    let binary = Path::new(env!("CARGO_BIN_EXE_notchling"));
    let options = SpawnOptions {
        wait: Duration::from_secs(30),
        data_root: Some(root),
    };
    assert_eq!(
        ensure_daemon_with(binary, &socket, &options).unwrap(),
        Started::Spawned
    );
    let old = state.join("notchlingd.log.1");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !old.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let moved = std::fs::metadata(&old).map(|m| m.len()).unwrap_or(0);
    let fresh = std::fs::metadata(&log).map(|m| m.len()).unwrap_or(u64::MAX);
    stop(&socket);
    assert!(moved >= big as u64, "the old log is {moved} bytes");
    assert!(fresh < 4096, "the new log is {fresh} bytes");
    assert!(std::fs::read_to_string(&log)
        .unwrap()
        .contains("the log passed 5 MB"));
}
