use std::ffi::OsStr;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::grant_files::{clock_ms, wipe_vec, GrantValue, MAX_FILE_BYTES, MAX_VALUE_BYTES};
use super::grant_files_sys::{verify_file_private, Dir, NO_UNIX};

pub const CLAUDE: &str = "claude";
pub const CLUSTER_LOGIN_ENTRY: &str = "claude-worker";
pub const LOGINS_DIR: &str = "logins";
const FILE_VERSION: u32 = 1;

#[derive(Clone)]
pub struct LoginFile {
    pub tool: String,
    pub expires_ms: i64,
    pub value: GrantValue,
}

impl fmt::Debug for LoginFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginFile")
            .field("tool", &self.tool)
            .field("expires_ms", &self.expires_ms)
            .field("value", &"<hidden>")
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
struct Stored {
    v: u32,
    tool: String,
    #[serde(rename = "expiresMs")]
    expires_ms: i64,
    value: GrantValue,
}

fn check_tool(tool: &str) -> Result<(), String> {
    if tool == CLAUDE {
        Ok(())
    } else {
        Err("not a tool with a standing login".to_string())
    }
}

fn file_name(tool: &str) -> String {
    format!("{tool}.json")
}

pub fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_VALUE_BYTES
        && !value.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub struct LoginStore {
    root: PathBuf,
    lock: Mutex<()>,
}

fn read_login(mut file: std::fs::File, path: &Path) -> Result<Stored, String> {
    let bad = || format!("malformed login file {}", path.display());
    let size = file.metadata().map_or(0, |m| m.len()).min(MAX_FILE_BYTES);
    let mut buffer = Vec::with_capacity(size as usize + 1);
    let read = (&mut file)
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut buffer);
    let parsed = match read {
        Ok(n) if (n as u64) <= MAX_FILE_BYTES => {
            serde_json::from_slice::<Stored>(&buffer).map_err(|_| bad())
        }
        Ok(_) => Err(bad()),
        Err(e) => Err(format!("can't read login file {}: {e}", path.display())),
    };
    wipe_vec(&mut buffer);
    parsed
}

impl LoginStore {
    pub fn new(cluster_dir: &Path) -> LoginStore {
        LoginStore {
            root: cluster_dir.join(LOGINS_DIR),
            lock: Mutex::new(()),
        }
    }

    pub fn path_of(&self, tool: &str) -> PathBuf {
        self.root.join(file_name(tool))
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn root_dir(&self, create: bool) -> Result<Option<Dir>, String> {
        let fail = |e: std::io::Error| format!("logins folder {}: {e}", self.root.display());
        match Dir::open_root(&self.root) {
            Ok(dir) => Ok(Some(dir)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                Dir::create_root(&self.root).map_err(fail)?;
                Dir::open_root(&self.root).map(Some).map_err(fail)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(fail(e)),
        }
    }

    fn private(&self, root: &Dir) -> Result<(), String> {
        root.verify_private()
            .map_err(|e| format!("logins folder {}: {e}", root.path().display()))
    }

    pub fn put(&self, login: &LoginFile) -> Result<(), String> {
        if !cfg!(unix) {
            return Err(NO_UNIX.to_string());
        }
        check_tool(&login.tool)?;
        if !valid_token(login.value.expose()) {
            return Err(
                "a login value is empty, too large or has a space or control character".to_string(),
            );
        }
        if login.expires_ms <= clock_ms() {
            return Err("the login has already expired".to_string());
        }
        let _guard = self.guard();
        let Some(root) = self.root_dir(true)? else {
            return Err("logins folder is missing".to_string());
        };
        self.private(&root)?;
        let file = file_name(&login.tool);
        let path = root.path().join(&file);
        match root.open_file(OsStr::new(&file)) {
            Ok(Some(_)) => root
                .unlink(OsStr::new(&file))
                .map_err(|e| format!("can't replace login file {}: {e}", path.display()))?,
            Ok(None) => {}
            Err(e) => return Err(format!("login file {}: {e}", path.display())),
        }
        let stored = Stored {
            v: FILE_VERSION,
            tool: login.tool.clone(),
            expires_ms: login.expires_ms,
            value: login.value.clone(),
        };
        let mut bytes = Vec::with_capacity(login.value.expose().len() * 6 + 256);
        let encoded = serde_json::to_writer(&mut bytes, &stored)
            .map_err(|_| format!("can't encode login file {}", path.display()));
        let written = encoded.and_then(|()| {
            let mut out = root
                .create_file(OsStr::new(&file))
                .map_err(|e| format!("can't write login file {}: {e}", path.display()))?;
            let result = out.write_all(&bytes).and_then(|()| out.sync_all());
            if result.is_err() {
                let _ = root.unlink(OsStr::new(&file));
            }
            result.map_err(|e| format!("can't write login file {}: {e}", path.display()))
        });
        wipe_vec(&mut bytes);
        written
    }

    pub fn get(&self, tool: &str, now_ms: i64) -> Result<Option<LoginFile>, String> {
        if !cfg!(unix) {
            return Err(NO_UNIX.to_string());
        }
        check_tool(tool)?;
        let _guard = self.guard();
        let Some(root) = self.root_dir(false)? else {
            return Ok(None);
        };
        let file = file_name(tool);
        let path = root.path().join(&file);
        let opened = match root.open_file(OsStr::new(&file)) {
            Ok(Some(opened)) => opened,
            Ok(None) => return Ok(None),
            Err(e) => return Err(format!("login file {}: {e}", path.display())),
        };
        let private = verify_file_private(&opened);
        let stored = read_login(opened, &path)?;
        if stored.v != FILE_VERSION || stored.tool != tool {
            return Err(format!("malformed login file {}", path.display()));
        }
        if stored.expires_ms <= now_ms {
            root.unlink(OsStr::new(&file))
                .map_err(|e| format!("can't delete login file {}: {e}", path.display()))?;
            return Ok(None);
        }
        private.map_err(|e| format!("login file {}: {e}", path.display()))?;
        self.private(&root)?;
        if !valid_token(stored.value.expose()) {
            return Err(format!("malformed login file {}", path.display()));
        }
        Ok(Some(LoginFile {
            tool: stored.tool,
            expires_ms: stored.expires_ms,
            value: stored.value,
        }))
    }

    pub fn remove(&self, tool: &str) -> Result<(), String> {
        check_tool(tool)?;
        let _guard = self.guard();
        let Some(root) = self.root_dir(false)? else {
            return Ok(());
        };
        let file = file_name(tool);
        root.unlink(OsStr::new(&file)).map_err(|e| {
            format!(
                "can't delete login file {}: {e}",
                root.path().join(&file).display()
            )
        })
    }

    pub fn standing(&self, tool: &str, now_ms: i64) -> bool {
        matches!(self.get(tool, now_ms), Ok(Some(_)))
    }
}

#[cfg(test)]
#[path = "login_files_tests.rs"]
mod tests;
