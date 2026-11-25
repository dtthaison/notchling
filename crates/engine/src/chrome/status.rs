use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

use super::strict::parse_written;
use crate::sessions::jobs::parse_object;

pub const ON: &str = "--chrome";
pub const OFF: &str = "--no-chrome";
pub const DEFAULT_KEY: &str = "claudeInChromeDefaultEnabled";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    Flag,
    Default,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detected {
    pub on: Option<bool>,
    pub basis: Basis,
    pub default_enabled: Option<bool>,
}

pub fn flag_of(flags: &[String]) -> Option<bool> {
    flags.iter().rev().find_map(|f| match f.as_str() {
        ON => Some(true),
        OFF => Some(false),
        _ => None,
    })
}

pub fn with_flag(flags: &[String], on: bool) -> Vec<String> {
    let mut out: Vec<String> = flags
        .iter()
        .filter(|f| f.as_str() != ON && f.as_str() != OFF)
        .cloned()
        .collect();
    out.push(if on { ON } else { OFF }.to_string());
    out
}

pub fn default_enabled(claude_json: &Path) -> Option<bool> {
    let value = read_object(claude_json)?;
    value.get(DEFAULT_KEY).and_then(Value::as_bool)
}

pub fn detect(flags: Option<&[String]>, default_enabled: Option<bool>) -> Detected {
    match flags.and_then(flag_of) {
        Some(on) => Detected {
            on: Some(on),
            basis: Basis::Flag,
            default_enabled,
        },
        None => match (flags, default_enabled) {
            (Some(_), Some(on)) => Detected {
                on: Some(on),
                basis: Basis::Default,
                default_enabled,
            },
            _ => Detected {
                on: None,
                basis: Basis::Unknown,
                default_enabled,
            },
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RewriteError {
    Read(String),
    NoFlags,
    Write(String),
    Changed,
}

impl std::fmt::Display for RewriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RewriteError::Read(why) => write!(f, "Couldn't read the session's file: {why}"),
            RewriteError::NoFlags => write!(f, "The session's file lists no start flags."),
            RewriteError::Write(why) => write!(f, "Couldn't update the session's file: {why}"),
            RewriteError::Changed => write!(f, "The session's file changed while it was updated."),
        }
    }
}

fn read_object(path: &Path) -> Option<Value> {
    parse_object(&std::fs::read(path).ok()?)
}

pub fn read_flags(state: &Path) -> Option<Vec<String>> {
    let value = read_object(state)?;
    let list = value.get("respawnFlags")?.as_array()?;
    list.iter()
        .map(|f| f.as_str().map(str::to_string))
        .collect()
}

pub fn tasks_in_flight(state: &Path) -> Option<u32> {
    let value = read_object(state)?;
    let flight = value.get("inFlight")?.as_object()?;
    let count = |key: &str| {
        flight
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0)
    };
    Some(count("tasks").max(count("drainableMonitors")) + count("queued"))
}

pub fn rewrite_flags(state: &Path, on: bool) -> Result<(), RewriteError> {
    let flags = read_flags(state).ok_or(RewriteError::NoFlags)?;
    set_flags(state, &with_flag(&flags, on))
}

const SET_TRIES: usize = 4;

pub fn set_flags(state: &Path, flags: &[String]) -> Result<(), RewriteError> {
    let mut last = RewriteError::Write("The session's file kept changing.".to_string());
    for attempt in 0..SET_TRIES {
        match set_flags_once(state, flags) {
            Err(RewriteError::Changed) => {
                last = RewriteError::Changed;
                std::thread::sleep(std::time::Duration::from_millis(15 * (attempt as u64 + 1)));
            }
            other => return other,
        }
    }
    Err(last)
}

fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < bytes.len() && bytes[i] != b'"' {
        if bytes[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    i
}

fn skip_space(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

pub fn flags_span(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let end = skip_string(bytes, i);
                if depth == 1 && text.get(i + 1..end) == Some("respawnFlags") {
                    let colon = skip_space(bytes, end + 1);
                    if bytes.get(colon) == Some(&b':') {
                        let open = skip_space(bytes, colon + 1);
                        if bytes.get(open) != Some(&b'[') {
                            return None;
                        }
                        let mut level = 0;
                        let mut j = open;
                        while j < bytes.len() {
                            match bytes[j] {
                                b'"' => j = skip_string(bytes, j),
                                b'[' => level += 1,
                                b']' => {
                                    level -= 1;
                                    if level == 0 {
                                        return Some((open, j + 1));
                                    }
                                }
                                _ => {}
                            }
                            j += 1;
                        }
                        return None;
                    }
                }
                i = end;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    None
}

fn root_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let start = skip_space(bytes, 0);
    if bytes.get(start) != Some(&b'{') {
        return None;
    }
    let mut depth = 0i32;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => i = skip_string(bytes, i),
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn kept_tail(tail: &str) -> &str {
    if tail
        .bytes()
        .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
    {
        tail
    } else if tail.ends_with('\n') {
        "\n"
    } else {
        ""
    }
}

pub fn with_flags_text(text: &str, flags: &[String]) -> Option<String> {
    let (open, close) = flags_span(text)?;
    let end = root_end(text)?;
    if close > end {
        return None;
    }
    let list = serde_json::to_string(flags).ok()?;
    let out = format!(
        "{}{}{}{}",
        &text[..open],
        list,
        &text[close..end],
        kept_tail(&text[end..])
    );
    let parsed = parse_written(&out)?;
    let back: Vec<&str> = parsed
        .get("respawnFlags")?
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<&str>>>()?;
    let same = back.len() == flags.len() && back.iter().zip(flags).all(|(a, b)| *a == b.as_str());
    same.then_some(out)
}

fn write_private(
    path: &Path,
    body: &[u8],
    like: Option<std::fs::Permissions>,
) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let _ = std::fs::remove_file(path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(body)?;
    file.sync_all()?;
    if let Some(perms) = like {
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn set_flags_once(state: &Path, flags: &[String]) -> Result<(), RewriteError> {
    let link = std::fs::symlink_metadata(state).map_err(|e| RewriteError::Read(e.to_string()))?;
    if link.file_type().is_symlink() {
        return Err(RewriteError::Write(
            "The session's file is a link, so it was left alone.".to_string(),
        ));
    }
    let text = std::fs::read_to_string(state).map_err(|e| RewriteError::Read(e.to_string()))?;
    let value = parse_object(text.as_bytes())
        .ok_or_else(|| RewriteError::Read("The file is not a JSON object.".to_string()))?;
    if value
        .get("respawnFlags")
        .and_then(Value::as_array)
        .is_none()
    {
        return Err(RewriteError::NoFlags);
    }
    let body = with_flags_text(&text, flags).ok_or_else(|| {
        RewriteError::Write("Couldn't find the start flags in the file.".to_string())
    })?;
    let tmp = state.with_extension("json.notchling-tmp");
    let like = std::fs::metadata(state).ok().map(|m| m.permissions());
    if let Err(e) = write_private(&tmp, body.as_bytes(), like) {
        let _ = std::fs::remove_file(&tmp);
        return Err(RewriteError::Write(e.to_string()));
    }
    if std::fs::read_to_string(state).ok().as_deref() != Some(text.as_str()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(RewriteError::Changed);
    }
    std::fs::rename(&tmp, state).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        RewriteError::Write(e.to_string())
    })?;
    if let Some(parent) = state.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    #[serde(default)]
    pub at_ms: i64,
    pub on: bool,
    pub before: Vec<String>,
    pub wanted: Vec<String>,
}

pub const PENDING_FILE: &str = "notchling-chrome-pending.json";

pub fn pending_path(state: &Path) -> std::path::PathBuf {
    state.with_file_name(PENDING_FILE)
}

pub fn write_pending(state: &Path, pending: &Pending) -> Result<(), RewriteError> {
    let body = serde_json::to_vec(pending).map_err(|e| RewriteError::Write(e.to_string()))?;
    write_private(&pending_path(state), &body, None).map_err(|e| RewriteError::Write(e.to_string()))
}

pub fn read_pending(state: &Path) -> Option<Pending> {
    serde_json::from_value(read_object(&pending_path(state))?).ok()
}

pub fn clear_pending(state: &Path) {
    let _ = std::fs::remove_file(pending_path(state));
}

const MODES: [&str; 4] = ["default", "acceptEdits", "plan", "auto"];
const MODE_FLAG: &str = "--permission-mode";
const MODE_TAIL_BYTES: u64 = 512 * 1024;

pub fn permission_mode(transcript: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(transcript).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(MODE_TAIL_BYTES)))
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let last = text.lines().rev().find_map(|line| {
        let value = parse_object(line.as_bytes())?;
        if value.get("type")?.as_str()? != "permission-mode" {
            return None;
        }
        Some(
            value
                .get("permissionMode")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        )
    })?;
    MODES.contains(&last.as_str()).then_some(last)
}

pub fn with_mode(flags: &[String], mode: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for flag in flags {
        if skip {
            skip = false;
            continue;
        }
        if flag == MODE_FLAG {
            skip = true;
            continue;
        }
        if flag.starts_with("--permission-mode=") {
            continue;
        }
        out.push(flag.clone());
    }
    out.push(MODE_FLAG.to_string());
    out.push(mode.to_string());
    out
}
