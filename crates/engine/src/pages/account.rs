use crate::lanes::LaneStore;
use serde::Deserialize;
use std::io::Read;
use std::path::Path;

const MAX_CONFIG_BYTES: u64 = 64 * 1024 * 1024;

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
