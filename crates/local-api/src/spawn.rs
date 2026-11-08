//! Starting the daemon when it isn't there: the one place the app and the command line do it.
//!
//! [`ensure_daemon`] connects first; only when nobody listens does it start `<binary> daemon
//! --socket <socket>` detached and wait up to 3 s for the socket. A daemon that is alive when the
//! wait ends is left running (the call says [`ClientError::Timeout`]; the caller connects later);
//! one that exited is an error. It refuses a binary outside an app
//! bundle (`*.app/Contents/…`) or a cargo target folder (`target/[<triple>/]debug|release`, or a
//! moved one marked by `CACHEDIR.TAG`), and one not named `notchling`, so a test can never start
//! some other program, and the socket is always passed explicitly, so a daemon started for a test
//! socket keeps its lock, audit log and log beside that socket. It starts the canonical path it
//! checked, and passes on none of the starter's `NOTCHLING_*` variables (above all not
//! `NOTCHLING_APP_TIER_PATHS`).

use crate::client::{connect_raw, ClientError};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
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
    /// Passed to the daemon as `NOTCHLING_DATA_ROOT` (a fixture root). No other `NOTCHLING_*`
    /// variable reaches it: the starter's own are removed.
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
    allowed_binary(binary).is_some()
}

/// The canonical path of `binary` when [`spawn_allowed`] says yes. That path, not `binary`, is
/// the one to start, so a symlink retargeted after the check starts nothing new.
pub fn allowed_binary(binary: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(binary).ok()?;
    let named = real.file_stem() == Some(OsStr::new("notchling"))
        && matches!(real.extension().and_then(OsStr::to_str), None | Some("exe"));
    (named && (in_app_bundle(&real) || in_cargo_target_real(&real))).then_some(real)
}

/// Whether `binary` (resolved) sits in a cargo target folder: a build, not an install. The daemon
/// honors its dev knobs ([`crate::tier::APP_TIER_PATHS_ENV`]) only then.
pub fn in_cargo_target(binary: &Path) -> bool {
    std::fs::canonicalize(binary).is_ok_and(|real| in_cargo_target_real(&real))
}

fn normal_parts(real: &Path) -> Vec<&OsStr> {
    real.components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect()
}

fn in_app_bundle(real: &Path) -> bool {
    normal_parts(real).windows(2).any(|w| {
        w[0].to_str().is_some_and(|n| n.ends_with(".app")) && w[1] == OsStr::new("Contents")
    })
}

fn in_cargo_target_real(real: &Path) -> bool {
    let parts = normal_parts(real);
    let profile = |name: &OsStr| name == OsStr::new("debug") || name == OsStr::new("release");
    parts
        .windows(2)
        .any(|w| w[0] == OsStr::new("target") && profile(w[1]))
        || parts
            .windows(3)
            .any(|w| w[0] == OsStr::new("target") && profile(w[2]))
        || in_tagged_target(real, profile)
}

/// A cargo target folder moved elsewhere (`build.target-dir`, `CARGO_TARGET_DIR`) is still named by
/// the `CACHEDIR.TAG` cargo writes at its root: `<root>/[<triple>/]debug|release/notchling`.
fn in_tagged_target(real: &Path, profile: impl Fn(&OsStr) -> bool) -> bool {
    let Some(profile_dir) = real.parent() else {
        return false;
    };
    if !profile_dir.file_name().is_some_and(&profile) {
        return false;
    }
    let tagged = |dir: Option<&Path>| dir.is_some_and(|d| d.join("CACHEDIR.TAG").is_file());
    let root = profile_dir.parent();
    tagged(root) || tagged(root.and_then(Path::parent))
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
    let mut command = daemon_command(
        binary,
        socket,
        options,
        std::env::vars_os().map(|(name, _)| name),
    )?;
    // The socket's folder, made private if missing, so the log can go beside the socket.
    if let Some(parent) = socket.parent().filter(|p| !p.as_os_str().is_empty()) {
        if !parent.exists() {
            notchling_platform::files::ensure_private_dir(parent)
                .map_err(|e| ClientError::Io(io::Error::other(e)))?;
        }
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
            reap(child);
            return Ok(Started::Spawned);
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(ClientError::Io(io::Error::other(format!(
                "the daemon exited at once ({status})"
            ))));
        }
        if Instant::now() >= deadline {
            // Not answering yet is not the same as never: a daemon on a loaded machine or a long
            // list is only slow, and the caller (the app starts one once per run, then keeps
            // trying to connect) meets it when it is up. Killing it here would leave nothing to
            // meet. The wait ends; the daemon is not ours to end.
            reap(child);
            return Err(ClientError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Waits for the daemon to end on a thread of its own, so it never lingers as a zombie of ours.
fn reap(mut child: Child) {
    let _ = std::thread::Builder::new()
        .name("notchling-daemon-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
}

/// Every Notchling setting variable starts with this.
const KNOB_PREFIX: &str = "NOTCHLING_";

/// The command that starts the daemon; `inherited` names the variables this process has.
fn daemon_command(
    binary: &Path,
    socket: &Path,
    options: &SpawnOptions,
    inherited: impl IntoIterator<Item = OsString>,
) -> Result<Command, ClientError> {
    let Some(real) = allowed_binary(binary) else {
        return Err(ClientError::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "won't start {} as the daemon: only a notchling binary in an app bundle or a cargo target folder",
                binary.display()
            ),
        )));
    };
    let mut command = Command::new(real);
    command
        .arg("daemon")
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null());
    // No NOTCHLING_* variable passes from whoever starts it (an agent's shell may be the
    // starter): the tier variable always goes, and the data root only when the caller names one.
    command.env_remove(crate::tier::APP_TIER_PATHS_ENV);
    for name in inherited {
        if name.to_str().is_some_and(|n| n.starts_with(KNOB_PREFIX)) {
            command.env_remove(name);
        }
    }
    if let Some(root) = &options.data_root {
        command.env("NOTCHLING_DATA_ROOT", root);
    }
    Ok(command)
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

    #[test]
    fn a_relocated_cargo_target_folder_is_found_by_its_cachedir_tag() {
        let dir = TempDir::new("spawn-cachedir");
        let d = dir.path();
        file(d, "cargo-out/CACHEDIR.TAG");
        assert!(spawn_allowed(&file(d, "cargo-out/debug/notchling")));
        assert!(spawn_allowed(&file(
            d,
            "cargo-out/x86_64-unknown-linux-gnu/release/notchling"
        )));
        // Without the tag, a folder that merely has a debug/ inside is not a target folder.
        assert!(!spawn_allowed(&file(d, "plain/debug/notchling")));
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
    fn the_daemon_inherits_no_notchling_knob() {
        let dir = TempDir::new("spawn-env");
        let binary = file(dir.path(), "repo/target/debug/notchling");
        let socket = dir.path().join("notchlingd.sock");
        let inherited = [
            "PATH",
            "NOTCHLING_APP_TIER_PATHS",
            "NOTCHLING_DATA_ROOT",
            "HOME",
        ]
        .map(OsString::from);
        let command =
            daemon_command(&binary, &socket, &SpawnOptions::default(), inherited).unwrap();
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        // Removed, so the daemon never sees them; the rest is inherited untouched.
        assert!(envs.contains(&(OsStr::new(crate::tier::APP_TIER_PATHS_ENV), None)));
        assert!(envs.contains(&(OsStr::new("NOTCHLING_DATA_ROOT"), None)));
        assert!(!envs.iter().any(|(k, _)| *k == "PATH" || *k == "HOME"));
        // A data root the caller names is the only one passed.
        let options = SpawnOptions {
            data_root: Some(dir.path().join("root")),
            ..SpawnOptions::default()
        };
        let command = daemon_command(&binary, &socket, &options, []).unwrap();
        let root = dir.path().join("root");
        assert!(command
            .get_envs()
            .any(|(k, v)| k == "NOTCHLING_DATA_ROOT" && v == Some(root.as_os_str())));
        assert!(command
            .get_envs()
            .any(|(k, v)| k == crate::tier::APP_TIER_PATHS_ENV && v.is_none()));
    }

    #[cfg(unix)]
    #[test]
    fn the_canonical_path_that_was_checked_is_the_one_started() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new("spawn-canon");
        let real = file(dir.path(), "repo/target/debug/notchling");
        let link = dir.path().join("link/notchling");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink(&real, &link).unwrap();
        let socket = dir.path().join("notchlingd.sock");
        let command = daemon_command(&link, &socket, &SpawnOptions::default(), []).unwrap();
        assert_eq!(
            Path::new(command.get_program()),
            std::fs::canonicalize(&real).unwrap()
        );
    }

    /// Kills the process a test left running, whatever the test did.
    #[cfg(unix)]
    struct KillOnDrop(Option<libc::pid_t>);

    #[cfg(unix)]
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            if let Some(pid) = self.0 {
                // SAFETY: a process this test started (its own pid file named it).
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        }
    }

    /// A daemon that is alive but not listening when the wait ends is only slow, so it is left to
    /// finish: the app starts a daemon once per run and then keeps trying to connect, and a daemon
    /// killed at the deadline is a daemon it never gets.
    #[cfg(unix)]
    #[test]
    fn a_daemon_still_starting_when_the_wait_ends_is_left_running() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("spawn-slow");
        let d = dir.path();
        // A cargo target folder (by its tag) holding a "notchling" that never binds a socket.
        file(d, "cargo-out/CACHEDIR.TAG");
        let binary = file(d, "cargo-out/debug/notchling");
        let pid_file = d.join("pid");
        let script = format!(
            "#!/bin/sh\necho $$ > '{}'\nexec sleep 60\n",
            pid_file.display()
        );
        std::fs::write(&binary, script).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let socket = d.join("notchlingd.sock");
        let mut guard = KillOnDrop(None);

        let options = SpawnOptions {
            wait: Duration::from_millis(1500),
            data_root: None,
        };
        let err = ensure_daemon_with(&binary, &socket, &options).unwrap_err();
        assert!(matches!(err, ClientError::Timeout), "{err:?}");

        let deadline = Instant::now() + Duration::from_secs(10);
        while !pid_file.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
            .expect("the started program wrote its pid")
            .trim()
            .parse()
            .unwrap();
        guard.0 = Some(pid);
        // SAFETY: signal 0 only asks whether the process exists.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        assert!(alive, "the daemon was killed when the wait ended");
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
