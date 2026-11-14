#[cfg(target_os = "macos")]
use notchling_engine::logs::KeychainKeyStore;
use notchling_engine::logs::{FileKeyStore, KeyStore};
use notchling_engine::sessions::Roots;
use notchling_engine::vault::list::VaultIndex;
use notchling_local_api::lock::lock_path_for;
use notchling_local_api::{AUDIT_NAME, SOCKET_NAME};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const DATA_ROOT_ENV: &str = "NOTCHLING_DATA_ROOT";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub claude_dir: PathBuf,
    pub data_dir: PathBuf,
    pub socket: PathBuf,
    pub socket_given: bool,
    pub fixture: bool,
}

impl Layout {
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
        let usual = data_dir.join(SOCKET_NAME);
        let socket_given = socket.is_some_and(|s| s != usual);
        let socket = socket.map(Path::to_path_buf).unwrap_or(usual);
        Some(Layout {
            claude_dir,
            data_dir,
            socket,
            socket_given,
            fixture,
        })
    }

    pub fn from_env(socket: Option<&Path>) -> Option<Layout> {
        Layout::resolve(socket, std::env::var_os(DATA_ROOT_ENV).as_deref(), || {
            Some((
                notchling_platform::paths::home()?,
                notchling_platform::paths::data_dir()?,
            ))
        })
    }

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

    pub fn antigravity_dir(&self) -> PathBuf {
        let home = self
            .claude_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        home.join(if self.fixture { "gemini" } else { ".gemini" })
            .join("antigravity-cli")
    }

    pub fn antigravity_copy_dir(&self) -> PathBuf {
        self.data_dir.join("daemon").join("antigravity")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.socket
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn audit_path(&self) -> PathBuf {
        self.state_dir().join(AUDIT_NAME)
    }

    pub fn audit_dir(&self) -> PathBuf {
        self.state_dir().join("audit")
    }

    pub fn events_db_path(&self) -> PathBuf {
        self.state_dir().join("daemon").join("events.db")
    }

    pub fn salt_path(&self) -> PathBuf {
        self.state_dir().join("daemon").join("log-salt")
    }

    pub fn host_id_path(&self) -> PathBuf {
        self.state_dir().join("daemon").join("host-id")
    }

    pub fn bridge_dir(&self) -> PathBuf {
        self.data_dir
            .parent()
            .unwrap_or(&self.data_dir)
            .join("AgentConsole")
            .join("bridge")
    }

    pub fn remote_log_path(&self) -> PathBuf {
        self.bridge_dir()
            .join(notchling_engine::bridge::net::audit::FILE_NAME)
    }

    pub fn key_store(&self) -> Arc<dyn KeyStore> {
        #[cfg(target_os = "macos")]
        if !self.fixture && !self.socket_given {
            return Arc::new(KeychainKeyStore::for_data_dir(&self.data_dir));
        }
        Arc::new(FileKeyStore::new(self.state_dir().join("daemon")))
    }

    pub fn account_log_path(&self) -> PathBuf {
        self.data_dir.join("account-log.json")
    }

    pub fn log_path(&self) -> PathBuf {
        self.state_dir().join(crate::LOG_NAME)
    }

    pub fn vault_index(&self) -> VaultIndex {
        VaultIndex::in_data(&self.data_dir)
    }

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
        assert_eq!(
            real.codex_dir_with(Some(OsStr::new("codex"))),
            PathBuf::from("/Users/me/.codex")
        );
        assert_eq!(real.home(), PathBuf::from("/Users/me"));
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
    fn naming_the_usual_socket_is_not_choosing_another() {
        let l = Layout::resolve(
            Some(Path::new("/r/notchling/notchlingd.sock")),
            Some(OsStr::new("/r")),
            real,
        )
        .unwrap();
        assert_eq!(l.socket, PathBuf::from("/r/notchling/notchlingd.sock"));
        assert!(!l.socket_given);
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
