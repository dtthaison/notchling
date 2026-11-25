use super::Tool;
use crate::install::edit::{self, Seg};
use crate::install::jsonc::{self, Node};
use crate::install::render::Json;
use crate::install::toml_edit::{self, TomlEditError};
use crate::machines::shell::is_safe_machine_path;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const CODEX_DAEMON_KNOB_VERIFIED: bool = false;
pub const AGY_KNOB_VERIFIED: bool = false;

pub const CLAUDE_VAR: &str = "DISABLE_AUTOUPDATER";
pub const AGY_VAR: &str = "AGY_CLI_DISABLE_AUTO_UPDATE";
pub const CODEX_CHECK_KEY: &str = "check_for_update_on_startup";
pub const CODEX_DAEMON_KEY: &str = "daemon_auto_start";

const NOT_VERIFIED: &str = "not verified yet";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Knob {
    ClaudeDisableAutoupdater,
    CodexUpdateCheck,
    CodexDaemonAutoStart,
    AgyEnv,
}

impl Knob {
    pub fn tool(self) -> Tool {
        match self {
            Knob::ClaudeDisableAutoupdater => Tool::Claude,
            Knob::CodexUpdateCheck | Knob::CodexDaemonAutoStart => Tool::Codex,
            Knob::AgyEnv => Tool::Agy,
        }
    }

    pub fn verified(self) -> bool {
        match self {
            Knob::ClaudeDisableAutoupdater | Knob::CodexUpdateCheck => true,
            Knob::CodexDaemonAutoStart => CODEX_DAEMON_KNOB_VERIFIED,
            Knob::AgyEnv => AGY_KNOB_VERIFIED,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Knob::ClaudeDisableAutoupdater => "claude_disable_autoupdater",
            Knob::CodexUpdateCheck => "codex_update_check",
            Knob::CodexDaemonAutoStart => "codex_daemon_auto_start",
            Knob::AgyEnv => "agy_env",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    ConfigDir(String),
    LaneConfigDir(String),
    CodexHome(String),
    ServiceFile(String),
}

impl Place {
    pub fn file(&self) -> String {
        match self {
            Place::ConfigDir(dir) | Place::LaneConfigDir(dir) => format!("{dir}/settings.json"),
            Place::CodexHome(dir) => format!("{dir}/config.toml"),
            Place::ServiceFile(path) => path.clone(),
        }
    }

    pub fn base(&self) -> String {
        let file = self.file();
        file.rsplit('/').next().unwrap_or(&file).to_string()
    }

    pub fn is_service(&self) -> bool {
        matches!(self, Place::ServiceFile(_))
    }

    pub fn word(&self) -> &'static str {
        match self {
            Place::ConfigDir(_) => "config_dir",
            Place::LaneConfigDir(_) => "lane_config_dir",
            Place::CodexHome(_) => "codex_home",
            Place::ServiceFile(_) => "service_file",
        }
    }

    pub fn fits(&self, knob: Knob) -> bool {
        matches!(
            (knob, self),
            (
                Knob::ClaudeDisableAutoupdater,
                Place::ConfigDir(_) | Place::LaneConfigDir(_)
            ) | (
                Knob::CodexUpdateCheck | Knob::CodexDaemonAutoStart,
                Place::CodexHome(_)
            ) | (Knob::AgyEnv, Place::ServiceFile(_))
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KnobState {
    Set,
    NotSet,
    Other(String),
    CantRead(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnobPlan {
    pub knob: Knob,
    pub place: Place,
    pub shown: String,
    pub from_sha256: String,
    pub new_text: String,
}

pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn other(value: &str) -> KnobState {
    let word = match value {
        "0" => "0",
        "true" => "true",
        "false" => "false",
        _ => "other",
    };
    KnobState::Other(word.to_string())
}

fn claude_state(text: &str) -> KnobState {
    if jsonc::is_blank(text) {
        return KnobState::NotSet;
    }
    let Ok(root) = jsonc::parse(text) else {
        return KnobState::CantRead("not_json");
    };
    if root.members().is_none() {
        return KnobState::CantRead("not_an_object");
    }
    if root.count_key("env") > 1 {
        return KnobState::CantRead("duplicate_key");
    }
    let Some(env) = root.get("env") else {
        return KnobState::NotSet;
    };
    if env.members().is_none() {
        return KnobState::CantRead("env_not_an_object");
    }
    if env.count_key(CLAUDE_VAR) > 1 {
        return KnobState::CantRead("duplicate_key");
    }
    match env.get(CLAUDE_VAR) {
        None => KnobState::NotSet,
        Some(node) => match node.as_str() {
            Some("1") => KnobState::Set,
            Some(value) => other(value),
            None => KnobState::Other("other".to_string()),
        },
    }
}

fn toml_state(text: &str, table: Option<&str>, key: &str) -> KnobState {
    match toml_edit::get_bool(text, table, key) {
        Ok(None) => KnobState::NotSet,
        Ok(Some(false)) => KnobState::Set,
        Ok(Some(true)) => KnobState::Other("true".to_string()),
        Err(TomlEditError::NotABool) => KnobState::Other("other".to_string()),
        Err(TomlEditError::Duplicate) => KnobState::CantRead("duplicate_key"),
        Err(TomlEditError::Parse(_)) => KnobState::CantRead("not_toml"),
    }
}

fn unit_has(text: &str, var: &str, value: &str) -> bool {
    let wanted = format!("{var}={value}");
    text.lines().any(|line| {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("Environment=") else {
            return false;
        };
        rest.split_whitespace()
            .any(|token| token.trim_matches('"') == wanted)
    })
}

fn plist_has(text: &str, var: &str, value: &str) -> bool {
    let key = format!("<key>{var}</key>");
    let Some(at) = text.find(&key) else {
        return false;
    };
    text[at + key.len()..]
        .trim_start()
        .starts_with(&format!("<string>{value}</string>"))
}

pub fn service_state(knob: Knob, text: &str) -> KnobState {
    let (var, value) = match knob {
        Knob::ClaudeDisableAutoupdater => (CLAUDE_VAR, "1"),
        Knob::AgyEnv => (AGY_VAR, "true"),
        Knob::CodexUpdateCheck | Knob::CodexDaemonAutoStart => {
            return KnobState::CantRead("no_service_knob")
        }
    };
    if unit_has(text, var, value) || plist_has(text, var, value) {
        KnobState::Set
    } else if text.contains(var) {
        KnobState::Other("other".to_string())
    } else {
        KnobState::NotSet
    }
}

pub fn state(knob: Knob, text: &str) -> KnobState {
    match knob {
        Knob::ClaudeDisableAutoupdater => claude_state(text),
        Knob::CodexUpdateCheck => toml_state(text, None, CODEX_CHECK_KEY),
        Knob::CodexDaemonAutoStart => toml_state(text, Some("features"), CODEX_DAEMON_KEY),
        Knob::AgyEnv => service_state(knob, text),
    }
}

pub fn place_state(knob: Knob, place: &Place, text: &str) -> KnobState {
    if place.is_service() {
        service_state(knob, text)
    } else {
        state(knob, text)
    }
}

fn guard(knob: Knob, place: &Place) -> Result<(), String> {
    if !knob.verified() {
        return Err(NOT_VERIFIED.to_string());
    }
    if !is_safe_machine_path(&place.file()) {
        return Err("unsafe_path".to_string());
    }
    if place.is_service() {
        return Err("set_by_the_service_step".to_string());
    }
    if !place.fits(knob) {
        return Err("wrong_place".to_string());
    }
    Ok(())
}

fn toml_word(e: TomlEditError) -> String {
    e.to_string()
}

fn parse_root(text: &str) -> Result<Node, String> {
    jsonc::parse(text).map_err(|_| "not_json".to_string())
}

fn claude_off(text: &str) -> Result<String, String> {
    if jsonc::is_blank(text) {
        return Ok(format!(
            "{{\n  \"env\": {{\n    \"{CLAUDE_VAR}\": \"1\"\n  }}\n}}\n"
        ));
    }
    let root = parse_root(text)?;
    let value = Json::Str("1".to_string());
    let new = if root.get("env").is_none() {
        edit::append(
            text,
            &[],
            Some("env"),
            &Json::obj(vec![(CLAUDE_VAR, value)]),
        )
    } else {
        edit::set_member(text, &[edit::key("env")], CLAUDE_VAR, &value)
    }
    .map_err(|e| e.to_string())?;
    let mut want = jsonc::to_value(&root, text);
    let env = want
        .as_object_mut()
        .map(|o| o.entry("env").or_insert_with(|| json!({})))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "not_an_object".to_string())?;
    env.insert(CLAUDE_VAR.to_string(), json!("1"));
    let got = jsonc::to_value(&parse_root(&new)?, &new);
    if got != want {
        return Err("check_failed".to_string());
    }
    Ok(new)
}

fn claude_on(text: &str) -> Result<String, String> {
    let root = parse_root(text)?;
    let members = root.members().ok_or_else(|| "not_an_object".to_string())?;
    let env_at = members
        .iter()
        .position(|m| m.key == "env")
        .ok_or_else(|| "no_env".to_string())?;
    let env = &members[env_at].value;
    let env_members = env
        .members()
        .ok_or_else(|| "env_not_an_object".to_string())?;
    let at = env_members
        .iter()
        .position(|m| m.key == CLAUDE_VAR)
        .ok_or_else(|| "no_key".to_string())?;
    let alone = env_members.len() == 1;
    let new = if alone {
        edit::remove(text, &[], env_at)
    } else {
        edit::remove(text, &[Seg::Key("env".to_string())], at)
    }
    .map_err(|e| e.to_string())?;
    let mut want = jsonc::to_value(&root, text);
    if let Some(object) = want.as_object_mut() {
        if alone {
            object.remove("env");
        } else if let Some(env) = object.get_mut("env").and_then(Value::as_object_mut) {
            env.remove(CLAUDE_VAR);
        }
    }
    let got = jsonc::to_value(&parse_root(&new)?, &new);
    if got != want {
        return Err("check_failed".to_string());
    }
    Ok(new)
}

pub fn shown_off(knob: Knob, place: &Place) -> String {
    let base = place.base();
    match knob {
        Knob::ClaudeDisableAutoupdater => {
            format!("{base}: \"env\": {{\"{CLAUDE_VAR}\": \"1\"}}")
        }
        Knob::CodexUpdateCheck => format!("{base}: {CODEX_CHECK_KEY} = false"),
        Knob::CodexDaemonAutoStart => format!("{base}: [features] {CODEX_DAEMON_KEY} = false"),
        Knob::AgyEnv => format!("{base}: {AGY_VAR}=true"),
    }
}

pub fn shown_on(knob: Knob, place: &Place) -> String {
    let base = place.base();
    match knob {
        Knob::ClaudeDisableAutoupdater => format!("{base}: remove \"{CLAUDE_VAR}\": \"1\""),
        Knob::CodexUpdateCheck => format!("{base}: remove {CODEX_CHECK_KEY} = false"),
        Knob::CodexDaemonAutoStart => {
            format!("{base}: remove [features] {CODEX_DAEMON_KEY} = false")
        }
        Knob::AgyEnv => format!("{base}: remove {AGY_VAR}=true"),
    }
}

fn plan_of(knob: Knob, place: &Place, text: &str, shown: String, new_text: String) -> KnobPlan {
    KnobPlan {
        knob,
        place: place.clone(),
        shown,
        from_sha256: sha256_hex(text),
        new_text,
    }
}

pub fn plan_off(knob: Knob, place: &Place, text: &str) -> Result<Option<KnobPlan>, String> {
    guard(knob, place)?;
    match state(knob, text) {
        KnobState::CantRead(why) => return Err(why.to_string()),
        KnobState::Set => return Ok(None),
        KnobState::NotSet | KnobState::Other(_) => {}
    }
    let new_text = match knob {
        Knob::ClaudeDisableAutoupdater => claude_off(text)?,
        Knob::CodexUpdateCheck => {
            toml_edit::set_bool(text, None, CODEX_CHECK_KEY, false).map_err(toml_word)?
        }
        Knob::CodexDaemonAutoStart => {
            toml_edit::set_bool(text, Some("features"), CODEX_DAEMON_KEY, false)
                .map_err(toml_word)?
        }
        Knob::AgyEnv => return Err("set_by_the_service_step".to_string()),
    };
    Ok(Some(plan_of(
        knob,
        place,
        text,
        shown_off(knob, place),
        new_text,
    )))
}

pub fn plan_on(knob: Knob, place: &Place, text: &str) -> Result<Option<KnobPlan>, String> {
    guard(knob, place)?;
    match state(knob, text) {
        KnobState::CantRead(why) => return Err(why.to_string()),
        KnobState::NotSet | KnobState::Other(_) => return Ok(None),
        KnobState::Set => {}
    }
    let new_text = match knob {
        Knob::ClaudeDisableAutoupdater => claude_on(text)?,
        Knob::CodexUpdateCheck => {
            toml_edit::remove(text, None, CODEX_CHECK_KEY).map_err(toml_word)?
        }
        Knob::CodexDaemonAutoStart => {
            toml_edit::remove(text, Some("features"), CODEX_DAEMON_KEY).map_err(toml_word)?
        }
        Knob::AgyEnv => return Err("set_by_the_service_step".to_string()),
    };
    Ok(Some(plan_of(
        knob,
        place,
        text,
        shown_on(knob, place),
        new_text,
    )))
}
