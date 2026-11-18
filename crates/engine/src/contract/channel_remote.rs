use std::path::Path;

use super::channel::Opener;
use super::channel_draft::encode;

pub const STATE_FILE: &str = "antigravity_state.pbtxt";

const HOST: &str = "https://antigravity.google.com/r/";

const CHOOSER: &str = "https://accounts.google.com/AccountChooser";

pub fn is_uuid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let sizes = [8, 4, 4, 4, 12];
    parts.len() == sizes.len()
        && parts
            .iter()
            .zip(sizes)
            .all(|(part, size)| part.len() == size && part.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn installation_uuid(state: &str) -> Option<String> {
    state.lines().find_map(|line| {
        let value = line.trim().strip_prefix("installation_uuid:")?;
        let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
        is_uuid(value).then(|| value.to_owned())
    })
}

pub fn read_installation(root: &Path) -> Option<String> {
    installation_uuid(&std::fs::read_to_string(root.join(STATE_FILE)).ok()?)
}

pub fn page(uuid: &str, conversation: Option<&str>) -> String {
    let mut url = format!("{HOST}{uuid}-v2");
    if let Some(id) = conversation {
        url.push_str("?p=");
        url.push_str(&encode(&format!("c/{id}")));
    }
    url
}

pub fn link(uuid: &str, email: Option<&str>, conversation: Option<&str>) -> String {
    let target = page(uuid, conversation);
    match email.filter(|e| !e.trim().is_empty()) {
        Some(email) => format!(
            "{CHOOSER}?Email={}&continue={}",
            encode(email),
            encode(&target)
        ),
        None => target,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoInstallation;

pub fn session_of(row_id: &str) -> Option<&str> {
    row_id
        .strip_prefix("agy:")
        .or_else(|| row_id.strip_prefix("antigravity:"))
        .filter(|id| !id.is_empty() && !id.contains(['/', '\\']))
}

pub fn hand_over(
    opener: &mut dyn Opener,
    root: &Path,
    email: Option<&str>,
    row_id: &str,
    text: &str,
    in_panel: bool,
) -> Result<(), NoInstallation> {
    let uuid = read_installation(root).ok_or(NoInstallation)?;
    let url = link(&uuid, email, session_of(row_id));
    opener.copy(text);
    if in_panel {
        opener.panel(&url);
    } else {
        opener.open(&url);
    }
    Ok(())
}

#[cfg(test)]
#[path = "channel_remote_tests.rs"]
mod tests;
