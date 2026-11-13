use crate::client::{connect_raw, ClientError};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const START_WAIT: Duration = Duration::from_secs(3);

pub const LOG_NAME: &str = "notchlingd.log";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Started {
    Running,
    Spawned,
}

#[derive(Clone, Debug)]
pub struct SpawnOptions {
    pub wait: Duration,
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

pub fn spawn_allowed(binary: &Path) -> bool {
    allowed_binary(binary).is_some()
}

pub fn allowed_binary(binary: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(binary).ok()?;
    let named = real.file_stem() == Some(OsStr::new("notchling"))
        && matches!(real.extension().and_then(OsStr::to_str), None | Some("exe"));
    (named && (in_app_bundle(&real) || in_cargo_target_real(&real))).then_some(real)
}

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

pub fn ensure_daemon(binary: &Path, socket: &Path) -> Result<Started, ClientError> {
    ensure_daemon_with(binary, socket, &SpawnOptions::default())
}

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
            reap(child);
            return Err(ClientError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn reap(mut child: Child) {
    let _ = std::thread::Builder::new()
        .name("notchling-daemon-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
}

const KNOB_PREFIX: &str = "NOTCHLING_";

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
        assert!(envs.contains(&(OsStr::new(crate::tier::APP_TIER_PATHS_ENV), None)));
        assert!(envs.contains(&(OsStr::new("NOTCHLING_DATA_ROOT"), None)));
        assert!(!envs.iter().any(|(k, _)| *k == "PATH" || *k == "HOME"));
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

    #[cfg(unix)]
    #[test]
    fn a_daemon_still_starting_when_the_wait_ends_is_left_running() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("spawn-slow");
        let d = dir.path();
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
