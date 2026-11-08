//! Slash commands a session can run: Claude Code's built-ins, plus the user's, the project's and
//! enabled plugins' commands and skills (`SlashCommands`). Commands that open a menu go to a
//! terminal; the rest are typed into the session. Reading is from files only; the caller caches.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlashCommand {
    /// Without the slash, e.g. "clear" or "sd:approve".
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
}

/// Commands that open a menu in the session's terminal and write nothing to the transcript, so a
/// hidden attach shows nothing (`/plugin` logged "(no content)"). Chat runs them in its terminal sheet.
pub const MENU_COMMANDS: [&str; 17] = [
    "plugin",
    "mcp",
    "agents",
    "config",
    "hooks",
    "permissions",
    "resume",
    "rewind",
    "memory",
    "output-style",
    "add-dir",
    "status",
    "help",
    "doctor",
    "export",
    "login",
    "logout",
];

/// These take a value; only the bare command opens a menu. (Chat answers a bare /model itself.)
const MENU_WHEN_BARE: [&str; 2] = ["model", "effort"];

/// Commands built into Claude Code, with no file on disk to scan. Most names come from the
/// `slash_commands` list `claude -p --output-format stream-json` printed on 2.1.318; the rest are
/// interactive-only and missing from that list. Update it when Claude Code adds commands.
const BUILT_INS: [(&str, &str); 63] = [
    ("clear", "Clear the conversation and start fresh"),
    ("compact", "Summarise the conversation to free up context"),
    ("context", "Show how much context is used"),
    ("cost", "Show the cost of this session"),
    ("usage", "Show plan usage limits"),
    ("model", "Change the model"),
    ("effort", "Change the effort level"),
    ("fast", "Turn fast mode on or off"),
    ("review", "Review a pull request"),
    ("code-review", "Review the current diff or a pull request"),
    ("security-review", "Security review of pending changes"),
    ("ultrareview", "Deep multi-agent review in the cloud"),
    ("simplify", "Clean up the changed code"),
    ("init", "Write a CLAUDE.md for this project"),
    ("memory", "Edit memory files"),
    ("rewind", "Go back to an earlier point"),
    ("rename", "Rename this session"),
    ("resume", "Resume an earlier session"),
    ("export", "Export this conversation"),
    ("status", "Show version, model and account"),
    ("todos", "Show the current task list"),
    ("agents", "Manage subagents"),
    ("mcp", "Manage MCP servers"),
    ("plugin", "Manage plugins"),
    ("reload-plugins", "Reload plugins"),
    ("reload-skills", "Reload skills"),
    ("hooks", "Manage hooks"),
    ("permissions", "Manage tool permissions"),
    ("config", "Open settings"),
    ("update-config", "Change settings.json"),
    ("output-style", "Change the output style"),
    ("add-dir", "Add a working directory"),
    ("loop", "Run a prompt on a repeating interval"),
    ("schedule", "Schedule a cloud agent"),
    ("run", "Run the app to check a change"),
    (
        "fewer-permission-prompts",
        "Allow common read-only commands",
    ),
    ("claude-api", "Claude API reference"),
    ("plugin-authoring", "Make or change a plugin"),
    ("workflow-authoring", "Write a workflow script"),
    ("skill-doctor", "Check a skill for problems"),
    ("doctor", "Check the Claude Code install"),
    ("debug", "Debug this session"),
    ("insights", "Show usage insights"),
    ("recap", "Recap this session"),
    ("goal", ""),
    ("focus", ""),
    ("advisor", ""),
    ("batch", ""),
    ("color", ""),
    ("import", ""),
    ("autocompact", ""),
    ("auto-mode-setup", ""),
    ("design", ""),
    ("design-sync", ""),
    ("deep-research", ""),
    ("list-agents", ""),
    ("team-onboarding", ""),
    ("verify", ""),
    ("extra-usage", ""),
    ("usage-credits", ""),
    ("login", "Sign in"),
    ("logout", "Sign out"),
    ("help", "Show help"),
];

/// True when `name` (lower case) is a command that opens a menu.
pub fn is_menu_command(name: &str) -> bool {
    MENU_COMMANDS.contains(&name)
}

/// True when the text is a command that needs a terminal, like "/plugin" or a bare "/effort".
pub fn opens_menu(text: &str) -> bool {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return false;
    };
    // The text is trimmed, so a second word exists exactly when the command is not bare.
    let mut parts = rest.trim_start().splitn(2, char::is_whitespace);
    let Some(first) = parts.next().filter(|p| !p.is_empty()) else {
        return false;
    };
    let name = first.to_lowercase();
    let bare = parts.next().is_none();
    is_menu_command(&name) || (MENU_WHEN_BARE.contains(&name.as_str()) && bare)
}

/// Where a typed command goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Typed into the session like any reply.
    Session,
    /// Run in a terminal (`claude attach` in a visible pty): it opens a menu.
    Terminal,
}

pub fn route(text: &str) -> Route {
    if opens_menu(text) {
        Route::Terminal
    } else {
        Route::Session
    }
}

/// The built-in commands, menu ones marked "Opens a menu".
pub fn built_ins() -> Vec<SlashCommand> {
    BUILT_INS
        .iter()
        .map(|(name, description)| SlashCommand {
            name: (*name).to_string(),
            description: if is_menu_command(name) {
                if description.is_empty() {
                    "Opens a menu".to_string()
                } else {
                    format!("{description} · Opens a menu")
                }
            } else {
                (*description).to_string()
            },
            argument_hint: None,
        })
        .collect()
}

/// Every command a session in `cwd` offers: built-ins first, then user, project and plugin ones by
/// name; a later source never overrides an earlier name. `home` is the user's home folder.
pub fn all(home: &Path, cwd: &str) -> Vec<SlashCommand> {
    let mut found = scan(&home.join(".claude"), None);
    if !cwd.is_empty() {
        found.extend(scan(&Path::new(cwd).join(".claude"), None));
    }
    for (name, path) in enabled_plugins(home, cwd) {
        found.extend(scan(Path::new(&path), Some(&name)));
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    let mut seen = HashSet::new();
    built_ins()
        .into_iter()
        .chain(found)
        .filter(|c| seen.insert(c.name.clone()))
        .collect()
}

/// Commands matching what is typed after the slash: prefix matches first, then the rest.
pub fn matches(typed: &str, commands: &[SlashCommand], limit: usize) -> Vec<SlashCommand> {
    let q = typed.to_lowercase();
    let is_prefix = |c: &SlashCommand| {
        let name = c.name.to_lowercase();
        name.starts_with(&q)
            || name
                .split(':')
                .rfind(|p| !p.is_empty())
                .is_some_and(|last| last.starts_with(&q))
    };
    let prefix: Vec<&SlashCommand> = commands.iter().filter(|c| is_prefix(c)).collect();
    let rest: Vec<&SlashCommand> = if q.is_empty() {
        Vec::new()
    } else {
        commands
            .iter()
            .filter(|c| c.name.to_lowercase().contains(&q) && !is_prefix(c))
            .collect()
    };
    prefix
        .into_iter()
        .chain(rest)
        .take(limit)
        .cloned()
        .collect()
}

/// `<dir>/commands/*.md` and `<dir>/skills/<name>/SKILL.md`.
fn scan(dir: &Path, prefix: Option<&str>) -> Vec<SlashCommand> {
    let mut result = Vec::new();
    let commands_dir = dir.join("commands");
    for file in names_in(&commands_dir) {
        if let Some(stem) = file.strip_suffix(".md") {
            let meta = front_matter(&commands_dir.join(&file));
            result.push(make(stem, &meta, prefix));
        }
    }
    let skills_dir = dir.join("skills");
    for sub in names_in(&skills_dir) {
        let file = skills_dir.join(&sub).join("SKILL.md");
        if !file.is_file() {
            continue;
        }
        let meta = front_matter(&file);
        let name = meta.get("name").cloned().unwrap_or_else(|| sub.clone());
        result.push(make(&name, &meta, prefix));
    }
    result
}

fn names_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn make(name: &str, meta: &BTreeMap<String, String>, prefix: Option<&str>) -> SlashCommand {
    let mut description = meta.get("description").cloned().unwrap_or_default();
    // The first sentence, when it ends within 110 characters.
    if let Some((index, _)) = description.char_indices().find(|(_, c)| *c == '.') {
        if description[..index].chars().count() < 110 {
            description.truncate(index + 1);
        }
    }
    SlashCommand {
        name: prefix.map_or_else(|| name.to_string(), |p| format!("{p}:{name}")),
        description,
        argument_hint: meta.get("argument-hint").cloned(),
    }
}

/// The simple `key: value` lines between the opening `---` lines.
fn front_matter(path: &Path) -> BTreeMap<String, String> {
    use std::io::Read;
    let mut meta = BTreeMap::new();
    let Ok(file) = std::fs::File::open(path) else {
        return meta;
    };
    let mut bytes = Vec::new();
    if file.take(4096).read_to_end(&mut bytes).is_err() {
        return meta;
    }
    let text = String::from_utf8_lossy(&bytes);
    if !text.starts_with("---") {
        return meta;
    }
    for line in text.split('\n').filter(|l| !l.is_empty()).skip(1) {
        if line.starts_with("---") {
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let mut value = value.trim().to_string();
        let bytes = value.as_bytes();
        if bytes.len() >= 2
            && (bytes[0] == b'"' || bytes[0] == b'\'')
            && bytes[bytes.len() - 1] == bytes[0]
        {
            value = value[1..value.len() - 1].to_string();
        }
        meta.insert(key.trim().to_string(), value);
    }
    meta
}

fn read_object(path: &Path) -> Option<Value> {
    let data = std::fs::read(path).ok()?;
    serde_json::from_slice(&data).ok().filter(Value::is_object)
}

/// Plugin name to install path, for plugins turned on in settings. Like Claude Code, the project's
/// `.claude/settings.json` overrides the user's, and `settings.local.json` overrides both, so a
/// plugin installed with `--scope project` shows up in that folder's sessions.
pub fn enabled_plugins(home: &Path, cwd: &str) -> Vec<(String, String)> {
    let claude = home.join(".claude");
    let mut files: Vec<PathBuf> = vec![claude.join("settings.json")];
    if !cwd.is_empty() {
        let project = Path::new(cwd).join(".claude");
        files.push(project.join("settings.json"));
        files.push(project.join("settings.local.json"));
    }
    let mut enabled: BTreeMap<String, bool> = BTreeMap::new();
    for file in files {
        let Some(Value::Object(map)) = read_object(&file)
            .and_then(|v| v.get("enabledPlugins").cloned())
            .filter(Value::is_object)
        else {
            continue;
        };
        for (key, on) in map {
            if let Some(on) = on.as_bool() {
                enabled.insert(key, on);
            }
        }
    }
    let Some(installed) = read_object(&claude.join("plugins/installed_plugins.json"))
        .and_then(|v| v.get("plugins").cloned())
    else {
        return Vec::new();
    };
    enabled
        .into_iter()
        .filter_map(|(key, on)| {
            if !on {
                return None;
            }
            let entries = installed.get(&key)?.as_array()?;
            let project_of = |e: &Value| {
                e.get("projectPath")
                    .and_then(Value::as_str)
                    .map(String::from)
            };
            // A project or local install records its folder; prefer this folder's, then a user install.
            let entry = entries
                .iter()
                .find(|e| project_of(e).as_deref() == Some(cwd))
                .or_else(|| entries.iter().find(|e| e.get("projectPath").is_none()))
                .or_else(|| entries.first())?;
            let path = entry.get("installPath")?.as_str()?;
            let name = key.split('@').next().unwrap_or(&key).to_string();
            Some((name, path.to_string()))
        })
        .collect()
}

#[cfg(test)]
#[path = "slash_tests.rs"]
mod tests;
