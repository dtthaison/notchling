//! Which folders the daemon reads and where its socket goes.
//!
//! - `NOTCHLING_DATA_ROOT=<dir>` (the M1 fixture convention): `<dir>/claude` is read as `~/.claude`
//!   and `<dir>/notchling` is the data folder. A fixture root keeps finished rows however old,
//!   since a fixture's dates are fixed.
//! - Otherwise `~/.claude` and the OS's data folder (`platform::paths`).
//! - The socket is `<data dir>/notchlingd.sock` unless `--socket` names another. The socket's lock
//!   and the audit log sit beside the socket.
//! - One daemon per data folder: over the real folders, a daemon whose socket is elsewhere also
//!   locks `<data dir>/notchlingd.lock` ([`Layout::data_lock`]), so `--socket` can't start a
//!   second one over the same folders. Under a fixture root only the socket's lock is taken, so a
//!   daemon given a test socket writes nothing in the root.

use notchling_engine::sessions::Roots;
use notchling_local_api::lock::lock_path_for;
use notchling_local_api::{AUDIT_NAME, SOCKET_NAME};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The variable naming a fixture root.
pub const DATA_ROOT_ENV: &str = "NOTCHLING_DATA_ROOT";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Read as `~/.claude`.
    pub claude_dir: PathBuf,
    /// Notchling's data folder (`lanes.json`).
    pub data_dir: PathBuf,
    pub socket: PathBuf,
    /// True when `--socket` chose the socket: its folder is then not ours to tighten.
    pub socket_given: bool,
    /// True for a fixture root.
    pub fixture: bool,
}

impl Layout {
    /// Works the layout out from `--socket`, the data root variable and the real folders
    /// (`home`, data folder), which are asked for only when there's no data root.
    pub fn resolve(
        socket: Option<&Path>,
        data_root: Option<&OsStr>,
        real: impl FnOnce() -> Option<(PathBuf, PathBuf)>,
    ) -> Option<Layout> {
        let (claude_dir, data_dir, fixture) = match data_root.filter(|v| !v.is_empty()) {
            Some(root) => {
                let root = PathBuf::from(root);
                (root.join("claude"), root.join("notchling"), true)
            }
            None => {
                let (home, data) = real()?;
                (home.join(".claude"), data, false)
            }
        };
        let socket_given = socket.is_some();
        let socket = socket
            .map(Path::to_path_buf)
            .unwrap_or_else(|| data_dir.join(SOCKET_NAME));
        Some(Layout {
            claude_dir,
            data_dir,
            socket,
            socket_given,
            fixture,
        })
    }

    /// From the process environment.
    pub fn from_env(socket: Option<&Path>) -> Option<Layout> {
        Layout::resolve(socket, std::env::var_os(DATA_ROOT_ENV).as_deref(), || {
            Some((
                notchling_platform::paths::home()?,
                notchling_platform::paths::data_dir()?,
            ))
        })
    }

    /// Codex's home: `<root>/codex` under a fixture root; otherwise `CODEX_HOME` (an absolute
    /// path) or `~/.codex`.
    pub fn codex_dir(&self) -> PathBuf {
        self.codex_dir_with(std::env::var_os("CODEX_HOME").as_deref())
    }

    pub fn codex_dir_with(&self, codex_home: Option<&OsStr>) -> PathBuf {
        let home = self.claude_dir.parent().unwrap_or(&self.claude_dir);
        if self.fixture {
            return self
                .data_dir
                .parent()
                .unwrap_or(&self.data_dir)
                .join("codex");
        }
        match codex_home.map(PathBuf::from) {
            Some(custom) if custom.is_absolute() => custom,
            _ => home.join(".codex"),
        }
    }

    /// Read as the home folder (`~`): the parent of `~/.claude`.
    pub fn home(&self) -> PathBuf {
        self.claude_dir
            .parent()
            .unwrap_or(&self.claude_dir)
            .to_path_buf()
    }

    pub fn roots(&self) -> Roots {
        Roots {
            main_dir: self.claude_dir.clone(),
            lanes_file: self.data_dir.join("lanes.json"),
        }
    }

    /// `~/.gemini/antigravity-cli`, which is only ever read through a copy. A fixture root keeps it
    /// at `<root>/gemini/antigravity-cli`, beside `claude`.
    pub fn antigravity_dir(&self) -> PathBuf {
        let home = self
            .claude_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        home.join(if self.fixture { "gemini" } else { ".gemini" })
            .join("antigravity-cli")
    }

    /// Where the copy of Antigravity's summaries database is made: ours, never Antigravity's, and
    /// the daemon's own folder (`<data dir>/daemon`, where its search index is too), not the Swift
    /// app's `<data dir>/antigravity`, whose `summaries.db` it would otherwise delete and rewrite.
    pub fn antigravity_copy_dir(&self) -> PathBuf {
        self.data_dir.join("daemon").join("antigravity")
    }

    /// The folder holding the socket, its lock and the audit log.
    pub fn state_dir(&self) -> PathBuf {
        self.socket
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn audit_path(&self) -> PathBuf {
        self.state_dir().join(AUDIT_NAME)
    }

    /// The lock that keeps one daemon per real data folder, when it isn't the socket's own lock:
    /// with `--socket` elsewhere a second daemon could otherwise read the same folders. A fixture
    /// root (`NOTCHLING_DATA_ROOT`) is a test's own, so it takes only the socket's lock.
    pub fn data_lock(&self) -> Option<PathBuf> {
        if self.fixture {
            return None;
        }
        let lock = lock_path_for(&self.data_dir.join(SOCKET_NAME));
        (lock_path_for(&self.socket) != lock).then_some(lock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Option<(PathBuf, PathBuf)> {
        Some((
            "/Users/me".into(),
            "/Users/me/Library/Application Support/Notchling".into(),
        ))
    }

    #[test]
    fn the_real_folders_and_the_default_socket() {
        let l = Layout::resolve(None, None, real).unwrap();
        assert_eq!(l.claude_dir, PathBuf::from("/Users/me/.claude"));
        assert_eq!(
            l.socket,
            PathBuf::from("/Users/me/Library/Application Support/Notchling/notchlingd.sock")
        );
        assert_eq!(l.audit_path(), l.data_dir.join("audit.jsonl"));
        assert!(!l.fixture && !l.socket_given);
    }

    #[test]
    fn antigravity_is_read_from_the_gemini_folder_and_copied_into_ours() {
        let real = Layout::resolve(None, None, real).unwrap();
        assert_eq!(
            real.antigravity_dir(),
            PathBuf::from("/Users/me/.gemini/antigravity-cli")
        );
        // Not `<data dir>/antigravity`, where the Swift app copies the same database under the same
        // file names: the daemon's own folder, like its search index.
        assert_eq!(
            real.antigravity_copy_dir(),
            real.data_dir.join("daemon").join("antigravity")
        );
        assert_ne!(
            real.antigravity_copy_dir(),
            real.data_dir.join("antigravity")
        );
        let fixture = Layout::resolve(None, Some(OsStr::new("/r")), || None).unwrap();
        assert_eq!(
            fixture.antigravity_dir(),
            PathBuf::from("/r/gemini/antigravity-cli")
        );
        assert_eq!(
            fixture.antigravity_copy_dir(),
            PathBuf::from("/r/notchling/daemon/antigravity")
        );
    }

    #[test]
    fn codex_lives_in_codex_home_else_dot_codex_and_beside_a_fixture_root() {
        let real = Layout::resolve(None, None, real).unwrap();
        assert_eq!(real.codex_dir_with(None), PathBuf::from("/Users/me/.codex"));
        assert_eq!(
            real.codex_dir_with(Some(OsStr::new("/elsewhere/codex"))),
            PathBuf::from("/elsewhere/codex")
        );
        // A relative CODEX_HOME is not followed.
        assert_eq!(
            real.codex_dir_with(Some(OsStr::new("codex"))),
            PathBuf::from("/Users/me/.codex")
        );
        assert_eq!(real.home(), PathBuf::from("/Users/me"));
        // A fixture root never reaches the real folders, whatever the environment says.
        let fixture = Layout::resolve(None, Some(OsStr::new("/r")), real_none).unwrap();
        assert_eq!(
            fixture.codex_dir_with(Some(OsStr::new("/Users/me/.codex"))),
            PathBuf::from("/r/codex")
        );
    }

    fn real_none() -> Option<(PathBuf, PathBuf)> {
        None
    }

    #[test]
    fn a_data_root_replaces_both_folders_and_never_asks_for_the_real_ones() {
        let l = Layout::resolve(None, Some(OsStr::new("/r")), || {
            panic!("read the real home")
        })
        .unwrap();
        assert_eq!(l.claude_dir, PathBuf::from("/r/claude"));
        assert_eq!(l.data_dir, PathBuf::from("/r/notchling"));
        assert_eq!(l.socket, PathBuf::from("/r/notchling/notchlingd.sock"));
        assert!(l.fixture);
        assert_eq!(
            l.roots().lanes_file,
            PathBuf::from("/r/notchling/lanes.json")
        );
    }

    #[test]
    fn an_explicit_socket_moves_the_lock_and_audit_with_it() {
        let l = Layout::resolve(
            Some(Path::new("/tmp/t/x.sock")),
            Some(OsStr::new("/r")),
            real,
        )
        .unwrap();
        assert_eq!(l.socket, PathBuf::from("/tmp/t/x.sock"));
        assert_eq!(l.audit_path(), PathBuf::from("/tmp/t/audit.jsonl"));
        assert!(l.socket_given);
    }

    #[test]
    fn the_real_data_folder_is_locked_even_with_a_socket_elsewhere() {
        let default = Layout::resolve(None, None, real).unwrap();
        assert_eq!(
            default.data_lock(),
            None,
            "the socket's lock is the data folder's"
        );
        let elsewhere = Layout::resolve(Some(Path::new("/tmp/t/x.sock")), None, real).unwrap();
        assert_eq!(
            elsewhere.data_lock(),
            Some(PathBuf::from(
                "/Users/me/Library/Application Support/Notchling/notchlingd.lock"
            ))
        );
        // A fixture root is the test's own: only the socket's lock.
        let fixture = Layout::resolve(
            Some(Path::new("/tmp/t/x.sock")),
            Some(OsStr::new("/r")),
            real,
        )
        .unwrap();
        assert_eq!(fixture.data_lock(), None);
    }

    #[test]
    fn an_empty_data_root_is_ignored_and_no_home_is_none() {
        let l = Layout::resolve(None, Some(OsStr::new("")), real).unwrap();
        assert!(!l.fixture);
        assert!(Layout::resolve(None, None, || None).is_none());
    }
}
