use notchling_engine::cluster::ready::HostFacts;
use notchling_engine::control::{Command, Launcher};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const EVERY: Duration = Duration::from_secs(600);
const FDESETUP: &str = "/usr/bin/fdesetup";
pub const TIMEOUT: Duration = Duration::from_secs(5);
const FREE_STEP_MB: u64 = 256;
const MAX_DEPTH: usize = 16;

pub type FreeMb = Box<dyn Fn(&Path) -> Option<u64> + Send + Sync>;

#[derive(Clone, Debug)]
pub struct Host {
    pub os: &'static str,
    pub home: PathBuf,
    pub user: String,
}

pub struct HostProbe {
    launcher: Arc<dyn Launcher>,
    host: Host,
    free: FreeMb,
    every: Duration,
    cache: Mutex<Option<(Instant, HostFacts)>>,
}

impl HostProbe {
    pub fn new(launcher: Arc<dyn Launcher>, host: Host, free: FreeMb) -> HostProbe {
        HostProbe {
            launcher,
            host,
            free,
            every: EVERY,
            cache: Mutex::new(None),
        }
    }

    pub fn with_every(mut self, every: Duration) -> HostProbe {
        self.every = every;
        self
    }

    pub fn facts(&self) -> HostFacts {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, facts)) = cache.as_ref() {
            if at.elapsed() < self.every {
                return facts.clone();
            }
        }
        let facts = self.read();
        *cache = Some((Instant::now(), facts.clone()));
        facts
    }

    fn read(&self) -> HostFacts {
        let free = (self.free)(&self.host.home).unwrap_or(0);
        HostFacts {
            encrypted: self.encrypted(),
            home: self.host.home.to_string_lossy().into_owned(),
            user: self.host.user.clone(),
            free_mb: free / FREE_STEP_MB * FREE_STEP_MB,
        }
    }

    fn encrypted(&self) -> Option<bool> {
        match self.host.os {
            "macos" => self.file_vault(),
            "linux" => self.luks(),
            _ => None,
        }
    }

    fn file_vault(&self) -> Option<bool> {
        let out = self
            .launcher
            .run(&Command::new(&[FDESETUP, "status"]), TIMEOUT)
            .ok()?;
        if out.code != 0 {
            return None;
        }
        file_vault_state(&out.output)
    }

    fn luks(&self) -> Option<bool> {
        let out = self
            .launcher
            .run(
                &Command::new(&["lsblk", "-o", "NAME,TYPE,MOUNTPOINT", "-J"]),
                TIMEOUT,
            )
            .ok()?;
        if out.code != 0 {
            return None;
        }
        home_on_crypt(&out.output, &self.host.home)
    }
}

pub fn file_vault_state(text: &str) -> Option<bool> {
    let line = text.lines().next()?.trim();
    match line {
        "FileVault is On." => Some(true),
        "FileVault is Off." => Some(false),
        _ => None,
    }
}

fn mounts_of(device: &Value) -> Vec<&str> {
    let mut out = Vec::new();
    if let Some(one) = device.get("mountpoint").and_then(Value::as_str) {
        out.push(one);
    }
    if let Some(many) = device.get("mountpoints").and_then(Value::as_array) {
        out.extend(many.iter().filter_map(Value::as_str));
    }
    out
}

fn walk(
    devices: &[Value],
    home: &Path,
    above: bool,
    depth: usize,
    best: &mut Option<(usize, bool)>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    for device in devices {
        let crypt = above || device.get("type").and_then(Value::as_str) == Some("crypt");
        for mount in mounts_of(device) {
            let path = Path::new(mount);
            if path.is_absolute()
                && home.starts_with(path)
                && best.is_none_or(|(len, _)| mount.len() > len)
            {
                *best = Some((mount.len(), crypt));
            }
        }
        if let Some(children) = device.get("children").and_then(Value::as_array) {
            walk(children, home, crypt, depth + 1, best);
        }
    }
}

pub fn home_on_crypt(json: &str, home: &Path) -> Option<bool> {
    let parsed: Value = serde_json::from_str(json).ok()?;
    let devices = parsed.get("blockdevices")?.as_array()?;
    let mut best = None;
    walk(devices, home, false, 0, &mut best);
    best.map(|(_, crypt)| crypt)
}

#[cfg(unix)]
pub fn free_mb_of(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    let bytes = (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64);
    Some(bytes / (1024 * 1024))
}

#[cfg(not(unix))]
pub fn free_mb_of(_path: &Path) -> Option<u64> {
    None
}

pub fn user_name(home: &Path) -> String {
    ["USER", "LOGNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .or_else(|| {
            home.file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

pub fn real(launcher: Arc<dyn Launcher>, home: PathBuf) -> HostProbe {
    let user = user_name(&home);
    HostProbe::new(
        launcher,
        Host {
            os: std::env::consts::OS,
            home,
            user,
        },
        Box::new(free_mb_of),
    )
}

#[cfg(test)]
#[path = "cluster_host_facts_tests.rs"]
mod tests;
