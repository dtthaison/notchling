use super::crypto::{constant_time_eq, hex_bytes, random_token, sha256, sha256_hex, uuid_v4};
use super::files::{iso8601, parse_iso8601, write_private, FileLock};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const FILE_NAME: &str = "devices.json";

pub const TOUCH_WRITE_MS: i64 = 60_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub token_hash: String,
    pub paired_at: i64,
    pub last_seen: Option<i64>,
}

#[derive(Debug)]
pub enum StoreError {
    Unreadable(String),
    Io(std::io::Error),
    Random,
    Stopped,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Unreadable(why) => {
                write!(f, "devices.json can't be read ({why}); not writing over it")
            }
            StoreError::Io(e) => write!(f, "{e}"),
            StoreError::Random => f.write_str("the system's random source failed"),
            StoreError::Stopped => f.write_str("the bridge is off"),
        }
    }
}

impl std::error::Error for StoreError {}

pub fn read_devices(file: &Path) -> Result<Vec<PairedDevice>, StoreError> {
    let data = match std::fs::read(file) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(StoreError::Io(e)),
    };
    decode(&data).map_err(StoreError::Unreadable)
}

pub fn decode(data: &[u8]) -> Result<Vec<PairedDevice>, String> {
    let value: Value = serde_json::from_slice(data).map_err(|e| e.to_string())?;
    let list = value.as_array().ok_or("not a list")?;
    list.iter().map(device_from).collect()
}

fn device_from(v: &Value) -> Result<PairedDevice, String> {
    let text = |key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("no {key}"))
    };
    let date = |key: &str| text(key).and_then(|t| parse_iso8601(&t).ok_or(format!("bad {key}")));
    let last_seen = match v.get("lastSeen") {
        None | Some(Value::Null) => None,
        Some(_) => Some(date("lastSeen")?),
    };
    Ok(PairedDevice {
        id: text("id")?,
        name: text("name")?,
        platform: text("platform")?,
        token_hash: text("tokenHash")?,
        paired_at: date("pairedAt")?,
        last_seen,
    })
}

pub fn encode(devices: &[PairedDevice]) -> Vec<u8> {
    if devices.is_empty() {
        return b"[\n\n]".to_vec();
    }
    let field = |key: &str, value: &str| format!("    \"{key}\" : {}", swift_string(value));
    let objects: Vec<String> = devices
        .iter()
        .map(|d| {
            let mut lines = vec![field("id", &d.id)];
            if let Some(seen) = d.last_seen {
                lines.push(field("lastSeen", &iso8601(seen)));
            }
            lines.push(field("name", &d.name));
            lines.push(field("pairedAt", &iso8601(d.paired_at)));
            lines.push(field("platform", &d.platform));
            lines.push(field("tokenHash", &d.token_hash));
            format!("  {{\n{}\n  }}", lines.join(",\n"))
        })
        .collect();
    format!("[\n{}\n]", objects.join(",\n")).into_bytes()
}

fn swift_string(text: &str) -> String {
    serde_json::to_string(text)
        .unwrap_or_else(|_| "\"\"".into())
        .replace('/', "\\/")
}

struct State {
    devices: Vec<PairedDevice>,
    seen: Option<(std::time::SystemTime, u64)>,
    readable: Result<(), String>,
    last_touch_write: HashMap<String, i64>,
    halted: bool,
}

pub struct DeviceStore {
    file: PathBuf,
    state: Mutex<State>,
}

impl DeviceStore {
    pub fn open(file: PathBuf) -> DeviceStore {
        let store = DeviceStore {
            file,
            state: Mutex::new(State {
                devices: Vec::new(),
                seen: None,
                readable: Ok(()),
                last_touch_write: HashMap::new(),
                halted: false,
            }),
        };
        store.refresh(&mut store.lock());
        store
    }

    pub fn halt(&self) {
        self.lock().halted = true;
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    pub fn devices(&self) -> Vec<PairedDevice> {
        let mut state = self.lock();
        self.refresh(&mut state);
        state.devices.clone()
    }

    pub fn register(
        &self,
        name: &str,
        platform: &str,
        now_ms: i64,
    ) -> Result<(PairedDevice, String), StoreError> {
        let token = random_token(32).map_err(|_| StoreError::Random)?;
        let device = PairedDevice {
            id: uuid_v4().map_err(|_| StoreError::Random)?,
            name: name.chars().take(80).collect(),
            platform: platform.chars().take(40).collect(),
            token_hash: sha256_hex(token.as_bytes()),
            paired_at: now_ms - now_ms.rem_euclid(1000),
            last_seen: None,
        };
        let added = device.clone();
        self.change(move |list| {
            list.push(added);
            true
        })?;
        Ok((device, token))
    }

    pub fn authenticate(&self, token: &str) -> Option<PairedDevice> {
        let candidate = sha256(token.as_bytes());
        let mut state = self.lock();
        self.refresh(&mut state);
        let mut found = None;
        for device in &state.devices {
            if constant_time_eq(&candidate, &hex_bytes(&device.token_hash)) {
                found = Some(device.clone());
            }
        }
        found
    }

    pub fn revoke(&self, id: &str) -> Result<Option<PairedDevice>, StoreError> {
        let mut removed = None;
        self.change(|list| match list.iter().position(|d| d.id == id) {
            Some(index) => {
                removed = Some(list.remove(index));
                true
            }
            None => false,
        })?;
        Ok(removed)
    }

    pub fn touch(&self, id: &str, now_ms: i64) -> Result<(), StoreError> {
        let write = {
            let mut state = self.lock();
            if state.halted {
                return Ok(());
            }
            let Some(device) = state.devices.iter_mut().find(|d| d.id == id) else {
                return Ok(());
            };
            device.last_seen = Some(now_ms);
            let due = state
                .last_touch_write
                .get(id)
                .is_none_or(|last| now_ms - last >= TOUCH_WRITE_MS);
            if due {
                state.last_touch_write.insert(id.to_string(), now_ms);
            }
            due
        };
        if !write {
            return Ok(());
        }
        self.change(|list| match list.iter_mut().find(|d| d.id == id) {
            Some(device) => {
                device.last_seen = Some(now_ms - now_ms.rem_euclid(1000));
                true
            }
            None => false,
        })
    }

    fn change(&self, edit: impl FnOnce(&mut Vec<PairedDevice>) -> bool) -> Result<(), StoreError> {
        if self.lock().halted {
            return Err(StoreError::Stopped);
        }
        let _lock = FileLock::acquire(&self.file).map_err(StoreError::Io)?;
        let mut state = self.lock();
        if state.halted {
            return Err(StoreError::Stopped);
        }
        state.seen = None;
        self.refresh(&mut state);
        if let Err(why) = &state.readable {
            return Err(StoreError::Unreadable(why.clone()));
        }
        let mut list = state.devices.clone();
        if !edit(&mut list) {
            return Ok(());
        }
        write_private(&self.file, &encode(&list)).map_err(StoreError::Io)?;
        state.devices = list;
        state.seen = stamp(&self.file);
        Ok(())
    }

    fn refresh(&self, state: &mut State) {
        let now = stamp(&self.file);
        if now.is_some() && now == state.seen {
            return;
        }
        match read_devices(&self.file) {
            Ok(mut fresh) => {
                for device in &mut fresh {
                    if let Some(old) = state.devices.iter().find(|d| d.id == device.id) {
                        device.last_seen = device.last_seen.max(old.last_seen);
                    }
                }
                state.devices = fresh;
                state.readable = Ok(());
            }
            Err(e) => {
                state.devices = Vec::new();
                state.readable = Err(e.to_string());
            }
        }
        state.seen = now;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn stamp(file: &Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(file).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

#[cfg(test)]
#[path = "devices_tests.rs"]
mod tests;
