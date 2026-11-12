//! The account a publish is saved under: the one in use where the session runs. A session in a
//! lane publishes as the lane's account (`lanes.json`, found from the `CLAUDE_CONFIG_DIR` the hook
//! inherits); any other session as the main login, which is `oauthAccount` in `~/.claude.json`
//! (the Swift app's `activeEmail`). Reading only: no login or token is touched.

use crate::lanes::LaneStore;
use serde::Deserialize;
use std::io::Read;
use std::path::Path;

/// `~/.claude.json` holds every project Claude Code has seen, so it can be large; it is read no
/// further than this.
const MAX_CONFIG_BYTES: u64 = 64 * 1024 * 1024;

/// The email of the account in use for a session whose `CLAUDE_CONFIG_DIR` is `config_dir`:
/// `home` is read as `~`, `data_dir` is Notchling's data folder (`lanes.json`). `""` when unknown.
pub fn active_email(home: &Path, data_dir: &Path, config_dir: Option<&str>) -> String {
    if let Some(lane) = LaneStore::new(home, data_dir).current(config_dir) {
        return lane.email;
    }
    main_email(&home.join(".claude.json"))
}

#[derive(Deserialize)]
struct Config {
    #[serde(rename = "oauthAccount")]
    account: Option<Profile>,
}

#[derive(Deserialize)]
struct Profile {
    #[serde(rename = "emailAddress")]
    email: Option<String>,
}

/// `oauthAccount.emailAddress` of the main login's `~/.claude.json`; `""` when it isn't there.
fn main_email(path: &Path) -> String {
    let Ok(file) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut data = Vec::new();
    if file.take(MAX_CONFIG_BYTES).read_to_end(&mut data).is_err() {
        return String::new();
    }
    serde_json::from_slice::<Config>(&data)
        .ok()
        .and_then(|c| c.account)
        .and_then(|a| a.email)
        .unwrap_or_default()
}
