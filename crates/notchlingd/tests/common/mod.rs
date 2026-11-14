#![allow(dead_code)]

pub mod audit;

use notchling_local_api::{Client, ClientKind};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Dir(pub PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn scratch(tag: &str) -> Dir {
    let dir = PathBuf::from(format!("/tmp/nl-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

pub fn copy_tree(from: &Path, to: &Path) {
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

pub fn fixture_root(dir: &Path) -> PathBuf {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/fixtures/notch");
    let root = dir.join("root");
    copy_tree(&fixture, &root);
    root
}

pub fn home(dir: &Path) -> PathBuf {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    home
}

pub fn socket(root: &Path) -> PathBuf {
    root.join("notchling/notchlingd.sock")
}

pub struct Daemon {
    child: Option<Child>,
}

fn alive(pid: libc::pid_t) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

impl Daemon {
    pub fn start(root: &Path, home: &Path) -> Daemon {
        Daemon::spawn(root, home, false)
    }

    pub fn start_for_app(root: &Path, home: &Path) -> Daemon {
        Daemon::spawn(root, home, true)
    }

    fn spawn(root: &Path, home: &Path, app: bool) -> Daemon {
        let mut command = Command::new(env!("CARGO_BIN_EXE_notchling"));
        command
            .arg("daemon")
            .env("NOTCHLING_DATA_ROOT", root)
            .env("HOME", home)
            .env_remove(notchling_local_api::APP_TIER_PATHS_ENV);
        if app {
            command.env(
                notchling_local_api::APP_TIER_PATHS_ENV,
                std::env::current_exe().unwrap(),
            );
        }
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let socket = socket(root);
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
                return Daemon { child: Some(child) };
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let mut child = child;
        let _ = child.kill();
        panic!("no daemon on {}", socket.display());
    }

    pub fn stop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let pid = child.id() as libc::pid_t;
        unsafe { libc::kill(pid, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(10);
        while alive(pid) && Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn connect(root: &Path, kind: ClientKind) -> Client {
    Client::connect(&socket(root), kind, "0.1.0").unwrap()
}

pub fn notchling(root: &Path, home: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_notchling"))
        .args(args)
        .env("NOTCHLING_DATA_ROOT", root)
        .env("HOME", home)
        .env_remove(notchling_local_api::APP_TIER_PATHS_ENV)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}
