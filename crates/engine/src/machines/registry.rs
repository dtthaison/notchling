//! The machines on disk (Swift `MachineRegistry`): `machines.json`, v2 with jobs.
//!
//! v2 is `{"version": 2, "machines": [...]}`, pretty-printed with sorted keys. Each machine needs
//! `name`, `host` and `user`; everything else takes its default. v1 (plan C) was the bare array of
//! machines with `python`, `sdPath`, `fileVault` and `repos` inline.
//!
//! The registry reads one file and writes another ([`RegistryFiles`]): until handover the Rust
//! tool reads the Swift app's `machines.json` and never writes it; its own writes go to
//! `<data>/daemon/machines/machines.json`, which it reads from then on. The first load of a v1
//! file copies its bytes to `<write file>.v1-backup` (never over an existing backup; a stamped name
//! when that one is taken) and writes v2. If the backup can't be written nothing is written. A file
//! that exists but can't be read, isn't JSON, has another version, or has an unknown job kind is
//! [`RegistryState::Unreadable`] and is never written over.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MachineOs {
    #[serde(rename = "macos")]
    MacOs,
    #[serde(rename = "linux")]
    Linux,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdminAccess {
    #[serde(rename = "sudo")]
    Sudo,
    #[serde(rename = "root-ssh")]
    RootSsh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockerState {
    None,
    Stopped,
    Rootful,
    Rootless,
}

/// What the Ability checks found on a machine. Every field may be missing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineAbilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xcode: Option<String>,
    #[serde(default)]
    pub ios_runtimes: Vec<String>,
    #[serde(default)]
    pub simulators: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker: Option<DockerState>,
    #[serde(
        default,
        rename = "freeDiskGB",
        skip_serializing_if = "Option::is_none"
    )]
    pub free_disk_gb: Option<i64>,
}

/// A repository a worker machine serves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineRepo {
    /// `owner/name`
    pub github: String,
    /// The checkout on this Mac.
    pub local_path: String,
    pub check_command: String,
    /// The checkout on the machine; `None` means `~/src/<name>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_path: Option<String>,
    /// A path inside the checkout, e.g. `.claude/skills/verify-lingeva/SKILL.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_skill: Option<String>,
}

fn python3() -> String {
    "python3".into()
}

/// The settings of a machine's sd worker job. Every key may be missing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdWorkerSettings {
    /// Absolute on the machine; `python3` until Base tools finds it.
    #[serde(default = "python3")]
    pub python: String,
    /// Absolute path of `sd.py` on the machine; Plugins installed sets it.
    #[serde(default)]
    pub sd_path: String,
    #[serde(default)]
    pub repos: Vec<MachineRepo>,
    /// `ios`, `backend`.
    #[serde(default)]
    pub kinds: Vec<String>,
    #[serde(default, rename = "healthURL", skip_serializing_if = "Option::is_none")]
    pub health_url: Option<String>,
    /// `YYYY-MM-DD`, from Token scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_expires: Option<String>,
    /// The vault name of the issues token: `<machine>-github-issues`. Empty in a file means the
    /// machine names it after itself.
    #[serde(default)]
    pub issues_secret: String,
    /// One issues token for all repositories; written only when true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_all_repos: Option<bool>,
}

impl SdWorkerSettings {
    pub fn new(machine: &str) -> SdWorkerSettings {
        SdWorkerSettings {
            python: python3(),
            sd_path: String::new(),
            repos: Vec::new(),
            kinds: Vec::new(),
            health_url: None,
            token_expires: None,
            issues_secret: format!("{machine}-github-issues"),
            token_all_repos: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobKind {
    #[serde(rename = "sd-worker")]
    SdWorker,
}

/// One job a machine does. One job per kind per machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub kind: JobKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sd_worker: Option<SdWorkerSettings>,
}

fn claude_worker() -> String {
    "claude-worker".into()
}

/// A machine Notchling sets up and watches over Tailscale SSH.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub name: String,
    /// Tailscale name.
    pub host: String,
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<MachineOs>,
    /// A FileVault machine waits for an unlock after a restart.
    #[serde(default)]
    pub file_vault: bool,
    /// Missing in a file: root over ssh on Linux (the worker can't be the admin there), sudo else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    admin: Option<AdminAccess>,
    #[serde(default = "claude_worker")]
    pub claude_secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abilities: Option<MachineAbilities>,
    #[serde(default)]
    pub jobs: Vec<Job>,
}

impl Machine {
    pub fn new(name: &str, host: &str, user: &str) -> Machine {
        Machine {
            name: name.into(),
            host: host.into(),
            user: user.into(),
            os: None,
            file_vault: false,
            admin: Some(AdminAccess::Sudo),
            claude_secret: claude_worker(),
            abilities: None,
            jobs: Vec::new(),
        }
    }

    /// How admin steps run on this machine.
    pub fn admin(&self) -> AdminAccess {
        self.admin.unwrap_or(match self.os {
            Some(MachineOs::Linux) => AdminAccess::RootSsh,
            _ => AdminAccess::Sudo,
        })
    }

    pub fn set_admin(&mut self, access: AdminAccess) {
        self.admin = Some(access);
    }

    /// Fills in what a file leaves to the machine: the admin, and the issues secret's name.
    fn settle(&mut self) {
        self.admin = Some(self.admin());
        let name = self.name.clone();
        for job in &mut self.jobs {
            if let Some(s) = &mut job.sd_worker {
                if s.issues_secret.is_empty() {
                    s.issues_secret = format!("{name}-github-issues");
                }
            }
        }
    }

    /// The sd worker job's settings.
    pub fn sd_worker(&self) -> Option<&SdWorkerSettings> {
        self.jobs
            .iter()
            .find(|j| j.kind == JobKind::SdWorker)
            .and_then(|j| j.sd_worker.as_ref())
    }

    pub fn sd_worker_mut(&mut self) -> Option<&mut SdWorkerSettings> {
        self.jobs
            .iter_mut()
            .find(|j| j.kind == JobKind::SdWorker)
            .and_then(|j| j.sd_worker.as_mut())
    }

    /// Adds the job or replaces its settings; `None` removes it.
    pub fn set_sd_worker(&mut self, value: Option<SdWorkerSettings>) {
        let index = self.jobs.iter().position(|j| j.kind == JobKind::SdWorker);
        match (index, value) {
            (Some(i), Some(v)) => self.jobs[i].sd_worker = Some(v),
            (Some(i), None) => {
                self.jobs.remove(i);
            }
            (None, Some(v)) => self.jobs.push(Job {
                kind: JobKind::SdWorker,
                sd_worker: Some(v),
            }),
            (None, None) => {}
        }
    }

    pub fn python(&self) -> &str {
        self.sd_worker()
            .map(|s| s.python.as_str())
            .unwrap_or("python3")
    }

    pub fn sd_path(&self) -> &str {
        self.sd_worker().map(|s| s.sd_path.as_str()).unwrap_or("")
    }

    pub fn repos(&self) -> &[MachineRepo] {
        self.sd_worker().map(|s| s.repos.as_slice()).unwrap_or(&[])
    }

    pub fn is_linux(&self) -> bool {
        self.os == Some(MachineOs::Linux)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryState {
    /// No `machines.json`.
    Missing,
    /// The machines; `migrated` is true on the one load that turned a v1 file into v2.
    Loaded {
        machines: Vec<Machine>,
        migrated: bool,
    },
    /// The file exists but can't be read or decoded. It is never overwritten.
    Unreadable(String),
}

impl RegistryState {
    pub fn machines(&self) -> Vec<Machine> {
        match self {
            RegistryState::Loaded { machines, .. } => machines.clone(),
            _ => Vec::new(),
        }
    }
}

/// Plan C's machine: the sd worker's settings inline.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineV1 {
    name: String,
    host: String,
    user: String,
    #[serde(default = "python3")]
    python: String,
    sd_path: String,
    file_vault: bool,
    repos: Vec<MachineRepo>,
}

impl MachineV1 {
    fn machine(self) -> Machine {
        let mut m = Machine::new(&self.name, &self.host, &self.user);
        m.file_vault = self.file_vault;
        let mut job = SdWorkerSettings::new(&self.name);
        job.python = self.python;
        job.sd_path = self.sd_path;
        job.repos = self.repos;
        m.set_sd_worker(Some(job));
        m
    }
}

#[derive(Deserialize)]
struct FileV1 {
    machines: Vec<MachineV1>,
}

#[derive(Deserialize)]
struct FileV2 {
    machines: Vec<Machine>,
}

enum Raw {
    Missing,
    Unreadable(String),
    V1(Vec<Machine>, Vec<u8>),
    V2(Vec<Machine>),
}

/// The file read and the file written. Equal after handover; until then the read file is the
/// Swift app's, which is only ever read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryFiles {
    /// The Rust tool's own file (`<data>/daemon/machines/machines.json`).
    pub own: PathBuf,
    /// The Swift app's file, read while the own file doesn't exist.
    pub swift: Option<PathBuf>,
}

impl RegistryFiles {
    /// One file, read and written.
    pub fn single(path: &Path) -> RegistryFiles {
        RegistryFiles {
            own: path.to_path_buf(),
            swift: None,
        }
    }

    /// The file loads read: the own one once it exists, else the Swift app's.
    pub fn source(&self) -> &Path {
        match &self.swift {
            Some(swift) if !self.own.exists() => swift,
            _ => &self.own,
        }
    }

    /// Where the pending update offers are kept, next to the own file.
    pub fn offers_file(&self) -> PathBuf {
        self.own.with_file_name("pending-updates.json")
    }

    pub fn load(&self) -> RegistryState {
        let source = self.source().to_path_buf();
        match read(&source) {
            Raw::Missing => RegistryState::Missing,
            Raw::Unreadable(why) => RegistryState::Unreadable(why),
            Raw::V2(machines) => RegistryState::Loaded {
                machines,
                migrated: false,
            },
            Raw::V1(machines, original) => {
                let Some(backup) = write_backup(&original, &self.own) else {
                    return RegistryState::Loaded {
                        machines,
                        migrated: false,
                    };
                };
                if write(&machines, &self.own) {
                    return RegistryState::Loaded {
                        machines,
                        migrated: true,
                    };
                }
                // nothing was migrated: don't leave a stray copy
                let _ = std::fs::remove_file(backup);
                RegistryState::Loaded {
                    machines,
                    migrated: false,
                }
            }
        }
    }

    /// `[]` for a missing and an unreadable file.
    pub fn all(&self) -> Vec<Machine> {
        self.load().machines()
    }

    /// Writes v2 to the own file only when what loads read is missing or a readable v2 file. False
    /// means nothing was written.
    pub fn save(&self, machines: &[Machine]) -> bool {
        match read(self.source()) {
            Raw::Missing | Raw::V2(_) => write(machines, &self.own),
            Raw::V1(..) | Raw::Unreadable(_) => false,
        }
    }

    pub fn machine(&self, name: &str) -> Option<Machine> {
        self.all().into_iter().find(|m| m.name == name)
    }
}

/// `<file>.v1-backup`.
pub fn backup_path(file: &Path) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(".v1-backup");
    file.with_file_name(name)
}

fn read(path: &Path) -> Raw {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Raw::Missing,
        Err(e) => return Raw::Unreadable(e.to_string()),
    };
    if let Ok(v1) = serde_json::from_slice::<Vec<MachineV1>>(&data) {
        return Raw::V1(v1.into_iter().map(MachineV1::machine).collect(), data);
    }
    let Ok(Value::Object(header)) = serde_json::from_slice::<Value>(&data) else {
        return Raw::Unreadable("not a machines.json file".into());
    };
    let Some(version) = header.get("version").and_then(Value::as_i64) else {
        return Raw::Unreadable("not a machines.json file".into());
    };
    match version {
        1 => match serde_json::from_slice::<FileV1>(&data) {
            Ok(f) => Raw::V1(
                f.machines.into_iter().map(MachineV1::machine).collect(),
                data,
            ),
            Err(e) => Raw::Unreadable(e.to_string()),
        },
        2 => match serde_json::from_slice::<FileV2>(&data) {
            Ok(f) => Raw::V2(
                f.machines
                    .into_iter()
                    .map(|mut m| {
                        m.settle();
                        m
                    })
                    .collect(),
            ),
            Err(e) => Raw::Unreadable(e.to_string()),
        },
        other => Raw::Unreadable(format!("version {other} is not supported")),
    }
}

static WRITES: Mutex<()> = Mutex::new(());

/// Held around every read-modify-write of `machines.json` in this process: the facts a refresh
/// learned (a background thread) and the window's edits (the UI thread) each load, change and save
/// under it, so neither saves over what the other just wrote.
pub fn write_lock() -> MutexGuard<'static, ()> {
    WRITES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Pretty JSON with every object's keys sorted, and a final newline.
pub fn pretty_sorted(value: Value) -> Vec<u8> {
    let mut data = serde_json::to_vec_pretty(&sort_keys(value)).unwrap_or_else(|_| b"{}".to_vec());
    data.push(b'\n');
    data
}

/// `value` with every object's keys in sorted order (whatever map serde_json was built with).
pub fn sort_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(k, v)| (k, sort_keys(v)))
                    .collect(),
            )
        }
        Value::Array(list) => Value::Array(list.into_iter().map(sort_keys).collect()),
        other => other,
    }
}

fn write(machines: &[Machine], path: &Path) -> bool {
    let settled: Vec<Machine> = machines
        .iter()
        .map(|m| {
            let mut m = m.clone();
            m.settle();
            m
        })
        .collect();
    let Ok(list) = serde_json::to_value(&settled) else {
        return false;
    };
    let file = serde_json::json!({"version": 2, "machines": list});
    let Some(dir) = path.parent() else {
        return false;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    notchling_platform::files::write_private_no_follow(path, &pretty_sorted(file)).is_ok()
}

/// Copies the original bytes to `<file>.v1-backup`, or to `<file>.v1-backup-<unix seconds>` when
/// that exists. An existing backup is never overwritten. The backup's path, or `None`.
fn write_backup(original: &[u8], file: &Path) -> Option<PathBuf> {
    use std::io::Write;
    let base = backup_path(file);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut candidates = vec![
        base.clone(),
        PathBuf::from(format!("{}-{stamp}", base.display())),
    ];
    candidates.extend((2..=50).map(|n| PathBuf::from(format!("{}-{stamp}-{n}", base.display()))));
    std::fs::create_dir_all(file.parent()?).ok()?;
    for candidate in candidates {
        if candidate.exists() {
            continue;
        }
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
            .ok()?;
        if f.write_all(original).is_err() || std::fs::read(&candidate).ok()? != original {
            let _ = std::fs::remove_file(&candidate);
            return None;
        }
        return Some(candidate);
    }
    None
}
