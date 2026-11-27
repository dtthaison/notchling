use super::deps::Deps;
use crate::tools::acp::RealSpawner as AcpSpawner;
use crate::tools::codex::plan::{augmented_path, find_binary, path_with_binary_folder};
use crate::tools::codex::process::RealSpawner as CodexSpawner;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Folders {
    pub home: PathBuf,
    pub codex_home: PathBuf,
    pub antigravity: PathBuf,
    pub claude_json: PathBuf,
    pub path: String,
    pub network: bool,
    pub catalog_file: PathBuf,
    pub quota_dir: Option<PathBuf>,
}

const MODELS_TIMEOUT: Duration = Duration::from_secs(60);

pub fn deps(folders: &Folders, stop: Arc<AtomicBool>) -> Deps {
    let path = augmented_path(&folders.path, &folders.home);
    let codex_binary = find_binary(&folders.home, &path, Path::is_file);
    let path = match &codex_binary {
        Some(binary) => path_with_binary_folder(&path, binary),
        None => path,
    };
    Deps {
        codex_binary,
        codex: Arc::new(CodexSpawner),
        codex_home: folders.codex_home.clone(),
        codex_media_root: folders.codex_home.join("generated_images"),
        path,
        network: folders.network,
        catalog_file: Some(folders.catalog_file.clone()),
        notchling_binary: std::env::current_exe().unwrap_or_default(),
        agy_binary: crate::tools::agy::find_binary(&folders.home)
            .map(|p| p.to_string_lossy().into_owned()),
        agy: Arc::new(AcpSpawner::new(&folders.home)),
        agy_media_root: folders.antigravity.join("brain"),
        capture: Arc::new(|program, args| capture(program, args, MODELS_TIMEOUT)),
        depth: 1,
        stop,
        agy_trust: Some(super::agy_trust::Files {
            claude_json: folders.claude_json.clone(),
            settings: folders.antigravity.join("settings.json"),
        }),
        recorder: None,
        cancel_grace: Deps::CANCEL_GRACE,
        terminate_after: Deps::TERMINATE_AFTER,
        quota: folders.quota_dir.clone().map(|dir| super::quota::Source {
            dir,
            clock: crate::transcript::util::now_secs,
        }),
    }
}

pub const CAPTURE_LIMIT: usize = 4 * 1024 * 1024;

const CAPTURE_GRACE: Duration = Duration::from_secs(2);

#[cfg(unix)]
fn kill_group(pid: u32) {
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_group(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub fn capture(program: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().ok()?;
    let pid = child.id();
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + timeout;
    let mut data: Vec<u8> = Vec::new();
    let mut over = false;
    let mut status = None;
    let mut closed = false;
    let mut grace_until = None;
    loop {
        loop {
            match rx.try_recv() {
                Ok(chunk) => {
                    if data.len() + chunk.len() > CAPTURE_LIMIT {
                        over = true;
                    } else {
                        data.extend_from_slice(&chunk);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        if over {
            kill_group(pid);
            let _ = child.wait();
            return None;
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(done)) => {
                    status = Some(done);
                    grace_until = Some(Instant::now() + CAPTURE_GRACE);
                }
                Ok(None) => {}
                Err(_) => {
                    kill_group(pid);
                    let _ = child.wait();
                    return None;
                }
            }
        }
        if status.is_some() && closed {
            break;
        }
        let now = Instant::now();
        let out_of_time = match grace_until {
            Some(until) => now >= until,
            None => now >= deadline,
        };
        if out_of_time {
            kill_group(pid);
            if status.is_none() {
                let _ = child.wait();
                return None;
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    kill_group(pid);
    status
        .filter(|s| s.success())
        .map(|_| String::from_utf8_lossy(&data).into_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn capture_returns_what_a_successful_program_printed() {
        let out = capture(
            Path::new("/bin/sh"),
            &["-c", "echo one; echo two"],
            Duration::from_secs(10),
        );
        assert_eq!(out.as_deref(), Some("one\ntwo\n"));
    }

    #[test]
    fn a_failing_missing_or_hanging_program_gives_nothing() {
        let t = Duration::from_secs(10);
        assert_eq!(
            capture(Path::new("/bin/sh"), &["-c", "echo x; exit 3"], t),
            None
        );
        assert_eq!(capture(Path::new("/no/such/program"), &[], t), None);
        let began = Instant::now();
        assert_eq!(
            capture(
                Path::new("/bin/sh"),
                &["-c", "exec sleep 30"],
                Duration::from_millis(200)
            ),
            None
        );
        assert!(began.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_grandchild_holding_stdout_does_not_hang_the_read() {
        let began = Instant::now();
        let out = capture(
            Path::new("/bin/sh"),
            &["-c", "echo listed; (sleep 30 &) ; exit 0"],
            Duration::from_secs(20),
        );
        assert_eq!(out.as_deref(), Some("listed\n"));
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "{:?}",
            began.elapsed()
        );
    }

    #[test]
    fn a_program_that_never_ends_is_killed_with_its_children() {
        let began = Instant::now();
        let out = capture(
            Path::new("/bin/sh"),
            &["-c", "sleep 30 & sleep 30"],
            Duration::from_millis(300),
        );
        assert_eq!(out, None);
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "{:?}",
            began.elapsed()
        );
    }

    #[test]
    fn more_than_the_cap_gives_nothing() {
        let began = Instant::now();
        let out = capture(
            Path::new("/bin/sh"),
            &["-c", "yes aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
            Duration::from_secs(20),
        );
        assert_eq!(out, None);
        assert!(
            began.elapsed() < Duration::from_secs(15),
            "{:?}",
            began.elapsed()
        );
    }

    #[test]
    fn the_real_deps_find_nothing_in_an_empty_home() {
        let home = std::env::temp_dir().join(format!("nl-delegate-real-{}", std::process::id()));
        let folders = Folders {
            home: home.clone(),
            codex_home: home.join(".codex"),
            antigravity: home.join(".gemini/antigravity-cli"),
            claude_json: home.join(".claude.json"),
            path: String::new(),
            network: true,
            catalog_file: home.join("catalog.json"),
            quota_dir: None,
        };
        let d = deps(&folders, Arc::new(AtomicBool::new(false)));
        assert_eq!(d.codex_media_root, home.join(".codex/generated_images"));
        assert_eq!(d.agy_media_root, home.join(".gemini/antigravity-cli/brain"));
        assert!(d.path.contains("/opt/homebrew/bin"));
        assert!(d
            .codex_binary
            .as_ref()
            .is_none_or(|p| !p.starts_with(&home)));
    }
}
