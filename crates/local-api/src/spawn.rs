//! Starting the daemon when it isn't there: the one place the app and the command line do it.
//!
//! [`ensure_daemon`] connects first; only when nobody listens does it start `<binary> daemon
//! --socket <socket>` detached and wait up to 3 s for the socket. It refuses a binary outside an app
//! bundle (`*.app/Contents/…`) or a cargo target folder (`target/[<triple>/]debug|release`), and
//! one not named `notchling`, so a test can never start some other program, and the socket is
//! always passed explicitly, so a daemon started for a test socket keeps its lock, audit log and
//! log beside that socket.

use crate::client::{connect_raw, ClientError};
use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long [`ensure_daemon`] waits for a started daemon's socket.
pub const START_WAIT: Duration = Duration::from_secs(3);

/// The file the started daemon's output goes to, beside its socket.
pub const LOG_NAME: &str = "notchlingd.log";

/// What [`ensure_daemon`] found or did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Started {
    /// A daemon already listened.
    Running,
    /// This call started one and it now listens.
    Spawned,
}

/// Options for [`ensure_daemon_with`].
#[derive(Clone, Debug)]
pub struct SpawnOptions {
    pub wait: Duration,
    /// Passed to the daemon as `NOTCHLING_DATA_ROOT` (a fixture root), else it inherits ours.
    pub data_root: Option<PathBuf>,
}

impl Default for SpawnOptions {
    fn default() -> Self {
        SpawnOptions {
            wait: START_WAIT,
            data_root: None,
        }
    }
}

/// Whether `binary` may be started as the daemon: named `notchling`, inside an app bundle or a
/// cargo target folder. Checked on the canonical path, so a symlink counts as what it points at.
pub fn spawn_allowed(binary: &Path) -> bool {
    let Ok(real) = std::fs::canonicalize(binary) else {
        return false;
    };
    let named = real.file_stem() == Some(OsStr::new("notchling"))
        && matches!(real.extension().and_then(OsStr::to_str), None | Some("exe"));
    if !named {
        return false;
    }
    let parts: Vec<&OsStr> = real
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    let in_bundle = parts.windows(2).any(|w| {
        w[0].to_str().is_some_and(|n| n.ends_with(".app")) && w[1] == OsStr::new("Contents")
    });
    let profile = |name: &OsStr| name == OsStr::new("debug") || name == OsStr::new("release");
    let in_target = parts
        .windows(2)
        .any(|w| w[0] == OsStr::new("target") && profile(w[1]))
        || parts
            .windows(3)
            .any(|w| w[0] == OsStr::new("target") && profile(w[2]));
    in_bundle || in_target
}

/// [`ensure_daemon_with`] with the default options.
pub fn ensure_daemon(binary: &Path, socket: &Path) -> Result<Started, ClientError> {
    ensure_daemon_with(binary, socket, &SpawnOptions::default())
}

/// Connects to `socket`; when nobody listens, starts `binary daemon --socket <socket>` detached and
/// waits for it.
pub fn ensure_daemon_with(
    binary: &Path,
    socket: &Path,
    options: &SpawnOptions,
) -> Result<Started, ClientError> {
    match connect_raw(socket) {
        Ok(_) => return Ok(Started::Running),
        Err(ClientError::NotRunning(_)) => {}
        Err(e) => return Err(e),
    }
    if !spawn_allowed(binary) {
        return Err(ClientError::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "won't start {} as the daemon: only a notchling binary in an app bundle or a cargo target folder",
                binary.display()
            ),
        )));
    }
    let mut command = Command::new(binary);
    command
        .arg("daemon")
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null());
    if let Some(root) = &options.data_root {
        command.env("NOTCHLING_DATA_ROOT", root);
    }
    match log_file(socket) {
        Some((out, err)) => command.stdout(out).stderr(err),
        None => command.stdout(Stdio::null()).stderr(Stdio::null()),
    };
    detach(&mut command);
    let mut child = command.spawn().map_err(ClientError::Io)?;
    let deadline = Instant::now() + options.wait;
    loop {
        if connect_raw(socket).is_ok() {
            // Reap it whenever it ends, so it never lingers as a zombie of ours.
            let _ = std::thread::Builder::new()
                .name("notchling-daemon-reaper".into())
                .spawn(move || {
                    let _ = child.wait();
                });
            return Ok(Started::Spawned);
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(ClientError::Io(io::Error::other(format!(
                "the daemon exited at once ({status})"
            ))));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ClientError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The log beside the socket, opened twice (stdout and stderr), when its folder exists.
fn log_file(socket: &Path) -> Option<(std::fs::File, std::fs::File)> {
    let path = socket.parent()?.join(LOG_NAME);
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let out = options.open(&path).ok()?;
    let err = out.try_clone().ok()?;
    Some((out, err))
}

/// Its own process group, so a Ctrl-C in the terminal that started the app doesn't reach it.
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn detach(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn file(dir: &Path, rel: &str) -> PathBuf {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"bin").unwrap();
        path
    }

    #[test]
    fn only_a_notchling_in_a_bundle_or_target_may_start() {
        let dir = TempDir::new("spawn-allow");
        let d = dir.path();
        assert!(spawn_allowed(&file(
            d,
            "Notchling.app/Contents/MacOS/notchling"
        )));
        assert!(spawn_allowed(&file(d, "repo/target/debug/notchling")));
        assert!(spawn_allowed(&file(d, "repo/target/release/notchling")));
        assert!(spawn_allowed(&file(
            d,
            "repo/target/x86_64-pc-windows-msvc/release/notchling.exe"
        )));
        assert!(!spawn_allowed(&file(d, "bin/notchling")));
        assert!(!spawn_allowed(&file(d, "repo/target/debug/other")));
        assert!(!spawn_allowed(&file(d, "Notchling.app/notchling")));
        assert!(!spawn_allowed(
            &d.join("repo/target/debug/missing/notchling")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_into_a_target_folder_counts_as_its_target() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new("spawn-link");
        let real = file(dir.path(), "bin/notchling");
        let link = dir.path().join("target/debug/notchling");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink(&real, &link).unwrap();
        assert!(!spawn_allowed(&link));
    }

    #[test]
    fn a_refused_binary_is_never_started() {
        let dir = TempDir::new("spawn-refuse");
        let socket = dir.path().join("notchlingd.sock");
        let err = ensure_daemon(Path::new("/bin/sh"), &socket).unwrap_err();
        assert!(matches!(err, ClientError::Io(e) if e.kind() == io::ErrorKind::PermissionDenied));
        assert!(!socket.exists());
    }
}
