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

pub const VARIABLE: &str = "CLAUDE_CONFIG_DIR";
pub const SERVICE_PREFIX: &str = "Claude Code-credentials-";

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
    NotAscii,
    OutsideLanes,
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

pub fn service_for(path: &str) -> Result<String, LaneError> {
    if !path.is_ascii() {
        return Err(LaneError::NotAscii);
    }
    let digest = Sha256::digest(path.as_bytes());
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    Ok(format!("{SERVICE_PREFIX}{hex}"))
}

pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn start_command(lane: &Lane) -> String {
    format!("{VARIABLE}={} claude", quote(&lane.path))
}

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

#[derive(Clone, Debug)]
pub struct LaneStore {
    pub main_dir: PathBuf,
    pub main_config: PathBuf,
    pub lanes_dir: PathBuf,
    pub lanes_file: PathBuf,
}

impl LaneStore {
    pub fn new(home: &Path, data_dir: &Path) -> LaneStore {
        LaneStore {
            main_dir: home.join(".claude"),
            main_config: home.join(".claude.json"),
            lanes_dir: home.join(".claude-lanes"),
            lanes_file: data_dir.join("lanes.json"),
        }
    }

    pub fn kept_dir(&self) -> PathBuf {
        self.lanes_dir.join(".kept")
    }

    pub fn all(&self) -> Vec<Lane> {
        read_lanes(&self.lanes_file)
    }

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

    pub fn current(&self, config_dir: Option<&str>) -> Option<Lane> {
        config_dir
            .filter(|p| !p.is_empty())
            .and_then(|p| self.lane_at_path(p))
    }

    pub fn path_for(&self, name: &str) -> String {
        self.lanes_dir.join(name).to_string_lossy().into_owned()
    }

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

    pub fn set_profile(&self, lane: &Lane, profile: &Value) -> Result<(), LaneError> {
        let config = self.own_folder(lane)?.join(".claude.json");
        let text = match fs::read_to_string(&config) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".into(),
            Err(e) => return Err(io(e)),
        };
        let out = set_top_level(&text, "oauthAccount", profile)
            .map_err(|e| LaneError::BadConfig(e.to_string()))?;
        notchling_platform::files::write_private_no_follow(&config, out.as_bytes()).map_err(io)
    }

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
        fs::remove_dir_all(&dir).map_err(io)
    }

    pub fn login(&self, keys: &Keys, lane: &Lane) -> Result<Option<Login>, LaneError> {
        let service = service_for(&lane.path)?;
        Ok(keys
            .read_json(&service, &keys.user)?
            .and_then(|item| item.object("claudeAiOauth")))
    }

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
