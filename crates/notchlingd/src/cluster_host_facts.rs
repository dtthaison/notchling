use notchling_engine::cluster::ready::HostFacts;
use notchling_engine::control::{Command, Launcher};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const EVERY: Duration = Duration::from_secs(600);
pub const RETRY: Duration = Duration::from_secs(60);
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
}

impl HostProbe {
    pub fn new(launcher: Arc<dyn Launcher>, host: Host, free: FreeMb) -> HostProbe {
        HostProbe {
            launcher,
            host,
            free,
        }
    }

    pub fn facts(&self) -> HostFacts {
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

pub fn is_known(facts: &HostFacts) -> bool {
    facts.encrypted.is_some() && facts.free_mb > 0
}

pub struct HostWatch {
    seen: Arc<Mutex<Option<HostFacts>>>,
    stop: Option<Sender<()>>,
}

impl HostWatch {
    pub fn spawn(probe: Arc<HostProbe>, every: Duration, retry: Duration) -> HostWatch {
        let seen: Arc<Mutex<Option<HostFacts>>> = Arc::new(Mutex::new(None));
        let (stop, stopped) = channel::<()>();
        let slot = seen.clone();
        let spawned = std::thread::Builder::new()
            .name("host-facts".into())
            .spawn(move || loop {
                let facts = probe.facts();
                let wait = if is_known(&facts) { every } else { retry };
                *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(facts);
                if !matches!(stopped.recv_timeout(wait), Err(RecvTimeoutError::Timeout)) {
                    return;
                }
            });
        HostWatch {
            seen,
            stop: spawned.ok().map(|_| stop),
        }
    }

    pub fn facts(&self) -> Option<HostFacts> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn running(&self) -> bool {
        self.stop.is_some()
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

pub fn real(launcher: Arc<dyn Launcher>, home: PathBuf) -> HostWatch {
    let user = user_name(&home);
    let probe = HostProbe::new(
        launcher,
        Host {
            os: std::env::consts::OS,
            home,
            user,
        },
        Box::new(free_mb_of),
    );
    HostWatch::spawn(Arc::new(probe), EVERY, RETRY)
}

#[cfg(test)]
#[path = "cluster_host_facts_tests.rs"]
mod tests;
