//! Lanes (`LaneStore` from `Lanes/Lanes.swift`): a lane is a Claude Code config folder of its own,
//! `~/.claude-lanes/<name>`, used with `CLAUDE_CONFIG_DIR=<path>`. Claude Code then keeps the
//! lane's login in the Keychain item `Claude Code-credentials-<first 8 hex of sha256(path)>`, its
//! profile in `<path>/.claude.json`, and runs a background host of its own. What should be the same
//! everywhere (settings, skills, plugins, history…) is a symlink into `~/.claude` ([`SHARED`]).
//!
//! One account is live in one place at a time (the main login or one lane): renewing a login
//! retires its old refresh token, so a second copy would be signed out. The switching rules are in
//! [`crate::accounts::claude`]; this module is the folders, `lanes.json` and the lane's Keychain
//! item. Every path is a field of [`LaneStore`], never found from the environment here.

use crate::accounts::json_splice::set_top_level;
use crate::accounts::keychain::{KeychainError, Keys};
use crate::accounts::Login;
use crate::sessions::lanes::{read_lanes, standardize};
pub use crate::sessions::Lane;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

mod relink;
pub use relink::{relink, Relinked};

/// The variable a lane's sessions run with.
pub const VARIABLE: &str = "CLAUDE_CONFIG_DIR";
/// The prefix of a lane's Keychain service.
pub const SERVICE_PREFIX: &str = "Claude Code-credentials-";

/// Shared with `~/.claude` through symlinks. Everything else stays the lane's own: `.claude.json`
/// (profile), the background host (`daemon*`), `sessions`, `jobs`, caches, and the account's
/// policy files.
pub const SHARED: [&str; 16] = [
    "settings.json",
    "CLAUDE.md",
    "keybindings.json",
    "statusline.sh",
    "history.jsonl",
    "skills",
    "plugins",
    "agents",
    "commands",
    "output-styles",
    "projects",
    "file-history",
    "paste-cache",
    "tasks",
    "todos",
    "plans",
];

#[derive(Debug)]
pub enum LaneError {
    Keychain(KeychainError),
    Io(String),
    /// A lane path that isn't plain ASCII: the Swift app hashed its Unicode-normalized (NFC) form,
    /// which this port doesn't reproduce, so it refuses rather than touch the wrong item.
    NotAscii,
    /// A folder outside `~/.claude-lanes`, or one that is a link.
    OutsideLanes,
    /// `.claude.json` that isn't a JSON object.
    BadConfig(String),
}

impl std::fmt::Display for LaneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaneError::Keychain(e) => write!(f, "{e}"),
            LaneError::Io(e) => write!(f, "{e}"),
            LaneError::NotAscii => {
                f.write_str("lane folders with non-ASCII characters aren't supported yet")
            }
            LaneError::OutsideLanes => f.write_str("that folder isn't a lane folder"),
            LaneError::BadConfig(e) => write!(f, "the lane's .claude.json: {e}"),
        }
    }
}

impl From<KeychainError> for LaneError {
    fn from(e: KeychainError) -> Self {
        LaneError::Keychain(e)
    }
}

fn io(e: impl std::fmt::Display) -> LaneError {
    LaneError::Io(e.to_string())
}

/// The lane's Keychain service: `Claude Code-credentials-` and the first 8 hex digits of the
/// SHA-256 of the exact path string (Claude Code hashes the variable's value).
pub fn service_for(path: &str) -> Result<String, LaneError> {
    if !path.is_ascii() {
        return Err(LaneError::NotAscii);
    }
    let digest = Sha256::digest(path.as_bytes());
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    Ok(format!("{SERVICE_PREFIX}{hex}"))
}

/// A single-quoted shell word.
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// What starts Claude Code on the lane's login: `CLAUDE_CONFIG_DIR='<path>' claude`.
pub fn start_command(lane: &Lane) -> String {
    format!("{VARIABLE}={} claude", quote(&lane.path))
}

/// A short name from the email's first part, unique among `taken`. Plain ASCII letters and digits
/// (the Swift app also kept other letters; a lane folder here is ASCII so its Keychain name is
/// exact), others become `-`, at most 24 characters.
pub fn new_name(email: &str, taken: &[String]) -> String {
    let lower = email.to_lowercase();
    let first = lower.split('@').next().unwrap_or("");
    let base: String = first
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let base = base.trim_matches('-');
    let stem: String = if base.is_empty() {
        "lane".into()
    } else {
        base.chars().take(24).collect()
    };
    let mut name = stem.clone();
    let mut n = 2;
    while taken.contains(&name) {
        name = format!("{stem}-{n}");
        n += 1;
    }
    name
}

/// The lanes on disk and their logins.
#[derive(Clone, Debug)]
pub struct LaneStore {
    /// `~/.claude`, which the shared entries link to.
    pub main_dir: PathBuf,
    /// `~/.claude.json`, copied into a new lane.
    pub main_config: PathBuf,
    /// `~/.claude-lanes`, outside `~/.claude` so nothing that walks it meets the links.
    pub lanes_dir: PathBuf,
    /// `lanes.json` in Notchling's data folder (the hook reads it too).
    pub lanes_file: PathBuf,
}

impl LaneStore {
    /// The usual layout under `home` and the data folder.
    pub fn new(home: &Path, data_dir: &Path) -> LaneStore {
        LaneStore {
            main_dir: home.join(".claude"),
            main_config: home.join(".claude.json"),
            lanes_dir: home.join(".claude-lanes"),
            lanes_file: data_dir.join("lanes.json"),
        }
    }

    /// Where closed lanes' `.lane-old-*` copies go.
    pub fn kept_dir(&self) -> PathBuf {
        self.lanes_dir.join(".kept")
    }

    pub fn all(&self) -> Vec<Lane> {
        read_lanes(&self.lanes_file)
    }

    /// Writes `lanes.json` (0600, atomically).
    pub fn save(&self, lanes: &[Lane]) -> Result<(), LaneError> {
        if let Some(dir) = self.lanes_file.parent() {
            notchling_platform::files::ensure_private_dir(dir).map_err(io)?;
        }
        let data = serde_json::to_vec(lanes).map_err(io)?;
        notchling_platform::files::write_private_no_follow(&self.lanes_file, &data).map_err(io)
    }

    pub fn lane_for_account(&self, id: &str) -> Option<Lane> {
        self.all().into_iter().find(|l| l.account_id == id)
    }

    pub fn lane_at_path(&self, path: &str) -> Option<Lane> {
        let wanted = standardize(path);
        self.all()
            .into_iter()
            .find(|l| standardize(&l.path) == wanted)
    }

    /// The lane a process runs in, from its `CLAUDE_CONFIG_DIR` (a hook inherits its session's).
    /// `None` for the main login.
    pub fn current(&self, config_dir: Option<&str>) -> Option<Lane> {
        config_dir
            .filter(|p| !p.is_empty())
            .and_then(|p| self.lane_at_path(p))
    }

    /// The folder a new lane named `name` gets.
    pub fn path_for(&self, name: &str) -> String {
        self.lanes_dir.join(name).to_string_lossy().into_owned()
    }

    /// Makes the lane's folder: links to the shared parts of `~/.claude`, and a `.claude.json`
    /// copied from the main one (projects, trust, MCP servers) with the lane's account as
    /// `oauthAccount`.
    pub fn prepare(
        &self,
        lane: &Lane,
        profile: &Value,
        now_secs: u64,
    ) -> Result<Relinked, LaneError> {
        let dir = self.own_folder(lane)?;
        notchling_platform::files::ensure_private_dir(&dir).map_err(io)?;
        let relinked = relink(lane, &self.main_dir, now_secs);
        let config = dir.join(".claude.json");
        if config.exists() {
            self.set_profile(lane, profile)?;
            self.finish_onboarding(lane)?;
        } else {
            let main = fs::read_to_string(&self.main_config).unwrap_or_else(|_| "{}".into());
            let text = set_top_level(&main, "oauthAccount", profile)
                .or_else(|_| set_top_level("{}", "oauthAccount", profile))
                .map_err(|e| LaneError::BadConfig(e.to_string()))?;
            notchling_platform::files::write_private_no_follow(&config, text.as_bytes())
                .map_err(io)?;
        }
        Ok(relinked)
    }

    /// Sets the lane's account in its `.claude.json`, so `/status` names it. Only `oauthAccount`
    /// changes.
    pub fn set_profile(&self, lane: &Lane, profile: &Value) -> Result<(), LaneError> {
        let config = self.own_folder(lane)?.join(".claude.json");
        // Only a missing file starts empty: one that can't be read is never written over.
        let text = match fs::read_to_string(&config) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".into(),
            Err(e) => return Err(io(e)),
        };
        let out = set_top_level(&text, "oauthAccount", profile)
            .map_err(|e| LaneError::BadConfig(e.to_string()))?;
        notchling_platform::files::write_private_no_follow(&config, out.as_bytes()).map_err(io)
    }

    /// Copies the main login's first-run flags (`hasCompletedOnboarding`, `lastOnboardingVersion`)
    /// into the lane's `.claude.json` where it lacks them. A lane whose file Claude Code made
    /// itself has neither, so every `claude` there opened on the first-run theme screen, where a
    /// hidden one (Trust Folder) waits for a question that never comes. Only missing keys are
    /// set; a lane with no file yet is left to [`LaneStore::prepare`]. True when it wrote.
    pub fn finish_onboarding(&self, lane: &Lane) -> Result<bool, LaneError> {
        const FLAGS: [&str; 2] = ["hasCompletedOnboarding", "lastOnboardingVersion"];
        let config = self.own_folder(lane)?.join(".claude.json");
        let mut text = match fs::read_to_string(&config) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(io(e)),
        };
        let Ok(Value::Object(main)) = fs::read_to_string(&self.main_config)
            .map_err(|_| ())
            .and_then(|t| serde_json::from_str(&t).map_err(|_| ()))
        else {
            return Ok(false);
        };
        let Ok(Value::Object(own)) = serde_json::from_str::<Value>(&text) else {
            return Err(LaneError::BadConfig(".claude.json isn't an object".into()));
        };
        let mut wrote = false;
        for key in FLAGS {
            if let (None, Some(value)) = (own.get(key), main.get(key)) {
                text = set_top_level(&text, key, value)
                    .map_err(|e| LaneError::BadConfig(e.to_string()))?;
                wrote = true;
            }
        }
        if wrote {
            notchling_platform::files::write_private_no_follow(&config, text.as_bytes())
                .map_err(io)?;
        }
        Ok(wrote)
    }

    /// The lane's folder, when it is directly inside [`LaneStore::lanes_dir`] and not a link.
    fn own_folder(&self, lane: &Lane) -> Result<PathBuf, LaneError> {
        let dir = PathBuf::from(standardize(&lane.path));
        let lanes = PathBuf::from(standardize(&self.lanes_dir.to_string_lossy()));
        if dir.parent() != Some(lanes.as_path()) || dir.file_name().is_none() {
            return Err(LaneError::OutsideLanes);
        }
        if fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(LaneError::OutsideLanes);
        }
        Ok(dir)
    }

    /// Removes the lane's own folder. Links are removed as links; what they point to stays. The
    /// copies `relink` set aside (`<name>.lane-old-<time>`) are moved to `.kept/<lane>-<time>/`
    /// first, and when one can't be moved the folder stays.
    pub fn remove_folder(&self, lane: &Lane, now_secs: u64) -> Result<(), LaneError> {
        let dir = self.own_folder(lane)?;
        let Ok(entries) = fs::read_dir(&dir) else {
            return Ok(());
        };
        let old: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".lane-old-"))
            .map(|e| e.path())
            .collect();
        if !old.is_empty() {
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let kept = self.kept_dir().join(format!("{name}-{now_secs}"));
            fs::create_dir_all(&kept).map_err(io)?;
            for path in old {
                let Some(file) = path.file_name() else {
                    continue;
                };
                fs::rename(&path, kept.join(file)).map_err(io)?;
            }
        }
        // `remove_dir_all` removes a link as a link, never what it points at.
        fs::remove_dir_all(&dir).map_err(io)
    }

    /// The lane's `claudeAiOauth`, as Claude Code last wrote it (it renews the login in place).
    pub fn login(&self, keys: &Keys, lane: &Lane) -> Result<Option<Login>, LaneError> {
        let service = service_for(&lane.path)?;
        Ok(keys
            .read_json(&service, &keys.user)?
            .and_then(|item| item.object("claudeAiOauth")))
    }

    /// Puts a login in the lane, keeping anything else Claude Code stored in the item (MCP logins)
    /// byte for byte.
    pub fn set_login(&self, keys: &Keys, lane: &Lane, oauth: &Login) -> Result<(), LaneError> {
        let service = service_for(&lane.path)?;
        let text = keys
            .read_text(&service, &keys.user)?
            .map(|t| t.expose().to_string())
            .unwrap_or_else(|| "{}".into());
        let value = oauth.clone().into_value();
        let out = set_top_level(&text, "claudeAiOauth", &value)
            .or_else(|_| set_top_level("{}", "claudeAiOauth", &value))
            .map_err(|e| LaneError::BadConfig(e.to_string()))?;
        Ok(keys.write(&service, &keys.user, &out)?)
    }

    pub fn remove_login(&self, keys: &Keys, lane: &Lane) -> Result<bool, LaneError> {
        let service = service_for(&lane.path)?;
        Ok(keys.delete(&service, &keys.user)?)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
