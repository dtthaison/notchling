//! What this Mac has installed (Swift `MacFacts.load`): the plugin versions to put on a machine.
//! The parsing is pure and takes the files' text; [`load`] reads the two files under a home and
//! the marketplace manifest, each capped at 4 MiB. Missing or unreadable files give empty facts.

use super::step::{InstalledPlugin, MacFacts};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

const READ_CAP: u64 = 4 * 1024 * 1024;

/// The user-scope entry of each plugin in `installed_plugins.json` (version 2): `scope` `user`,
/// with a `version` and an `installPath`.
pub fn installed_plugins(json: &str) -> BTreeMap<String, InstalledPlugin> {
    let mut found = BTreeMap::new();
    let Ok(Value::Object(root)) = serde_json::from_str::<Value>(json) else {
        return found;
    };
    let Some(Value::Object(all)) = root.get("plugins") else {
        return found;
    };
    for (id, value) in all {
        let Some(entries) = value.as_array() else {
            continue;
        };
        let user = entries
            .iter()
            .find(|e| e.get("scope").and_then(Value::as_str) == Some("user"));
        let Some(user) = user else { continue };
        if let (Some(version), Some(path)) = (
            user.get("version").and_then(Value::as_str),
            user.get("installPath").and_then(Value::as_str),
        ) {
            found.insert(
                id.clone(),
                InstalledPlugin {
                    version: version.into(),
                    path: path.into(),
                },
            );
        }
    }
    found
}

/// The folder of the `system-design` directory marketplace in `known_marketplaces.json`: its
/// source path, else its install location. `None` when it isn't a directory marketplace.
pub fn system_design_dir(known_json: &str) -> Option<String> {
    let root: Value = serde_json::from_str(known_json).ok()?;
    let market = root.get("system-design")?;
    let source = market.get("source")?;
    if source.get("source").and_then(Value::as_str) != Some("directory") {
        return None;
    }
    source
        .get("path")
        .and_then(Value::as_str)
        .or_else(|| market.get("installLocation").and_then(Value::as_str))
        .map(str::to_string)
}

fn read_capped(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut data = Vec::new();
    file.take(READ_CAP).read_to_end(&mut data).ok()?;
    String::from_utf8(data).ok()
}

/// Reads `<home>/.claude/plugins/installed_plugins.json` and the `system-design` directory
/// marketplace in `known_marketplaces.json`, with that marketplace's
/// `.claude-plugin/marketplace.json`.
pub fn load(home: &Path) -> MacFacts {
    let plugins = home.join(".claude/plugins");
    let mut facts = MacFacts::default();
    if let Some(text) = read_capped(&plugins.join("installed_plugins.json")) {
        facts.plugins = installed_plugins(&text);
    }
    if let Some(dir) = read_capped(&plugins.join("known_marketplaces.json"))
        .as_deref()
        .and_then(system_design_dir)
    {
        let manifest = Path::new(&dir).join(".claude-plugin/marketplace.json");
        if let Some(text) = read_capped(&manifest) {
            facts.marketplace_json.insert("system-design".into(), text);
        }
    }
    facts
}
