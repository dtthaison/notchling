//! The machines on disk (Swift `MachineRegistry`): `machines.json`, v2 with jobs.
//!
//! v2 is `{"version": 2, "machines": [...]}`, pretty-printed with sorted keys. Each machine needs
//! `name`, `host` and `user`; everything else takes its default. v1 (plan C) was the bare array of
//! machines with `python`, `sdPath`, `fileVault` and `repos` inline.
//!
//! The registry reads from one place and writes to another ([`RegistryFiles`]): until handover the
//! Rust tool reads the Swift app's `machines.json` and never writes it; its own copy is the
//! [`REGISTRY_DOC`] document in `<data>/notchling.db` (this tool alone rewrote it whole, so it
//! moved into the database, decision 0014), changed in one transaction so the app and the daemon
//! never lose each other's save. Its old file, `<data>/daemon/machines/machines.json`, moves in
//! once. The first load of a v1 registry copies its bytes to `<old file>.v1-backup` (a file that
//! stays; never over an existing backup, a stamped name when that one is taken) and writes v2. If
//! the backup can't be written nothing is written. A registry that can't be read, isn't JSON, has
//! another version, or has an unknown job kind is [`RegistryState::Unreadable`] and is never
//! written over (an old file like that stays where it is). A render ([`RegistryFiles::read_only`])
//! never imports, migrates or saves.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::store;

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

/// The document in `notchling.db` that holds what `machines.json` used to (v2, as the file's text).
pub const REGISTRY_DOC: &str = "machines/registry";

/// Where the registry is read from and written to.
///
/// It is written to one place, the [`REGISTRY_DOC`] document in `<data>/notchling.db`
/// (`crate::store`, one transaction per change). It is read from, in this order: that document;
/// this tool's old file (`old_own`, `<data>/daemon/machines/machines.json`, moved into the
/// document the first time it is read, unless it can't be read, which leaves it where it is and
/// makes it the source); the Swift app's file (`swift`, only ever read, until handover).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryFiles {
    /// Notchling's data folder, where `notchling.db` is.
    pub data_dir: PathBuf,
    /// This tool's file before it kept the registry in the database; the v1 backup goes beside it.
    pub old_own: PathBuf,
    /// The Swift app's file, read while there is no document.
    pub swift: Option<PathBuf>,
    /// A render: nothing is imported, migrated or saved, whatever is there.
    pub read_only: bool,
}

impl RegistryFiles {
    /// The database in `data_dir` and the old file at `data_dir/machines.json`, no Swift file.
    pub fn single(data_dir: &Path) -> RegistryFiles {
        RegistryFiles {
            data_dir: data_dir.to_path_buf(),
            old_own: data_dir.join("machines.json"),
            swift: None,
            read_only: false,
        }
    }

    /// What loads read, for a line in the window: the database while it holds the registry, else
    /// the file read. Opens the database, so not for a frame.
    pub fn source(&self) -> PathBuf {
        if matches!(store::doc(&self.data_dir, REGISTRY_DOC), Ok(Some(_))) {
            return store::db_path(&self.data_dir);
        }
        if file_exists(&self.old_own) {
            return self.old_own.clone();
        }
        self.swift.clone().unwrap_or_else(|| self.old_own.clone())
    }

    /// Moves the old own file into the document, once. Best effort: a file that stays is read
    /// as the source.
    fn import_old(&self) {
        if self.read_only {
            return;
        }
        let _ = store::import_file(&self.data_dir, REGISTRY_DOC, &self.old_own, |text| {
            !matches!(parse(text.as_bytes()), Raw::Unreadable(_))
        });
    }

    /// The files, when there is no document: the old own file once it exists, else the Swift app's.
    fn read_files(&self) -> Raw {
        if file_exists(&self.old_own) {
            return read(&self.old_own);
        }
        match &self.swift {
            Some(swift) => read(swift),
            None => Raw::Missing,
        }
    }

    fn raw(&self) -> Raw {
        self.import_old();
        match store::doc(&self.data_dir, REGISTRY_DOC) {
            Ok(Some(text)) => parse(text.as_bytes()),
            Ok(None) => self.read_files(),
            Err(why) => Raw::Unreadable(why),
        }
    }

    pub fn load(&self) -> RegistryState {
        match self.raw() {
            Raw::Missing => RegistryState::Missing,
            Raw::Unreadable(why) => RegistryState::Unreadable(why),
            Raw::V2(machines) => RegistryState::Loaded {
                machines,
                migrated: false,
            },
            Raw::V1(machines, original) => {
                let loaded = |migrated| RegistryState::Loaded {
                    machines: machines.clone(),
                    migrated,
                };
                if self.read_only {
                    return loaded(false);
                }
                let Some(backup) = write_backup(&original, &self.old_own) else {
                    return loaded(false);
                };
                if self.write_migrated(&machines) {
                    return loaded(true);
                }
                // nothing was migrated: don't leave a stray copy
                let _ = std::fs::remove_file(backup);
                loaded(false)
            }
        }
    }

    /// Writes the migrated v1 machines as v2, unless a v2 registry got there first.
    fn write_migrated(&self, machines: &[Machine]) -> bool {
        let Some(text) = serialize(machines) else {
            return false;
        };
        store::update_doc(&self.data_dir, REGISTRY_DOC, |row| match row {
            Some(old) => match parse(old.as_bytes()) {
                Raw::V2(_) => Ok(Some(old)),
                Raw::Unreadable(why) => Err(why),
                Raw::V1(..) | Raw::Missing => Ok(Some(text)),
            },
            None => Ok(Some(text)),
        })
        .is_ok()
    }

    /// `[]` for a missing and an unreadable registry.
    pub fn all(&self) -> Vec<Machine> {
        self.load().machines()
    }

    /// Writes v2 to the document only when what loads read is missing or a readable v2 registry,
    /// checked in the same transaction as the write. False means nothing was written.
    pub fn save(&self, machines: &[Machine]) -> bool {
        if self.read_only {
            return false;
        }
        let Some(text) = serialize(machines) else {
            return false;
        };
        self.import_old();
        // a registry that is refused anyway makes no database
        if !store::exists(&self.data_dir)
            && matches!(self.read_files(), Raw::V1(..) | Raw::Unreadable(_))
        {
            return false;
        }
        store::update_doc(&self.data_dir, REGISTRY_DOC, |row| {
            let raw = match row {
                Some(old) => parse(old.as_bytes()),
                None => self.read_files(),
            };
            match raw {
                Raw::Missing | Raw::V2(_) => Ok(Some(text)),
                Raw::V1(..) | Raw::Unreadable(_) => {
                    Err("the registry is never written over".into())
                }
            }
        })
        .is_ok()
    }

    pub fn machine(&self, name: &str) -> Option<Machine> {
        self.all().into_iter().find(|m| m.name == name)
    }
}

fn file_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// `<file>.v1-backup`.
pub fn backup_path(file: &Path) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(".v1-backup");
    file.with_file_name(name)
}

fn read(path: &Path) -> Raw {
    match std::fs::read(path) {
        Ok(data) => parse(&data),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Raw::Missing,
        Err(e) => Raw::Unreadable(e.to_string()),
    }
}

/// A registry's bytes (a file's, or the document's text) as v1, v2 or unreadable.
fn parse(data: &[u8]) -> Raw {
    if let Ok(v1) = serde_json::from_slice::<Vec<MachineV1>>(data) {
        return Raw::V1(
            v1.into_iter().map(MachineV1::machine).collect(),
            data.to_vec(),
        );
    }
    let Ok(Value::Object(header)) = serde_json::from_slice::<Value>(data) else {
        return Raw::Unreadable("not a machines.json file".into());
    };
    let Some(version) = header.get("version").and_then(Value::as_i64) else {
        return Raw::Unreadable("not a machines.json file".into());
    };
    match version {
        1 => match serde_json::from_slice::<FileV1>(data) {
            Ok(f) => Raw::V1(
                f.machines.into_iter().map(MachineV1::machine).collect(),
                data.to_vec(),
            ),
            Err(e) => Raw::Unreadable(e.to_string()),
        },
        2 => match serde_json::from_slice::<FileV2>(data) {
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

/// The v2 registry's text for `machines`.
fn serialize(machines: &[Machine]) -> Option<String> {
    let settled: Vec<Machine> = machines
        .iter()
        .map(|m| {
            let mut m = m.clone();
            m.settle();
            m
        })
        .collect();
    let list = serde_json::to_value(&settled).ok()?;
    let file = serde_json::json!({"version": 2, "machines": list});
    String::from_utf8(pretty_sorted(file)).ok()
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
