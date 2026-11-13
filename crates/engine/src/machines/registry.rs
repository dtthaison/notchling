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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineRepo {
    pub github: String,
    pub local_path: String,
    pub check_command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_skill: Option<String>,
}

fn python3() -> String {
    "python3".into()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdWorkerSettings {
    #[serde(default = "python3")]
    pub python: String,
    #[serde(default)]
    pub sd_path: String,
    #[serde(default)]
    pub repos: Vec<MachineRepo>,
    #[serde(default)]
    pub kinds: Vec<String>,
    #[serde(default, rename = "healthURL", skip_serializing_if = "Option::is_none")]
    pub health_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_expires: Option<String>,
    #[serde(default)]
    pub issues_secret: String,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub name: String,
    pub host: String,
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<MachineOs>,
    #[serde(default)]
    pub file_vault: bool,
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

    pub fn admin(&self) -> AdminAccess {
        self.admin.unwrap_or(match self.os {
            Some(MachineOs::Linux) => AdminAccess::RootSsh,
            _ => AdminAccess::Sudo,
        })
    }

    pub fn set_admin(&mut self, access: AdminAccess) {
        self.admin = Some(access);
    }

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
    Missing,
    Loaded {
        machines: Vec<Machine>,
        migrated: bool,
    },
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

pub const REGISTRY_DOC: &str = "machines/registry";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryFiles {
    pub data_dir: PathBuf,
    pub old_own: PathBuf,
    pub swift: Option<PathBuf>,
    pub read_only: bool,
}

impl RegistryFiles {
    pub fn single(data_dir: &Path) -> RegistryFiles {
        RegistryFiles {
            data_dir: data_dir.to_path_buf(),
            old_own: data_dir.join("machines.json"),
            swift: None,
            read_only: false,
        }
    }

    pub fn source(&self) -> PathBuf {
        if matches!(store::doc(&self.data_dir, REGISTRY_DOC), Ok(Some(_))) {
            return store::db_path(&self.data_dir);
        }
        if file_exists(&self.old_own) {
            return self.old_own.clone();
        }
        self.swift.clone().unwrap_or_else(|| self.old_own.clone())
    }

    fn import_old(&self) {
        if self.read_only {
            return;
        }
        let _ = store::import_file(&self.data_dir, REGISTRY_DOC, &self.old_own, |text| {
            !matches!(parse(text.as_bytes()), Raw::Unreadable(_))
        });
    }

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
                let _ = std::fs::remove_file(backup);
                loaded(false)
            }
        }
    }

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

    pub fn all(&self) -> Vec<Machine> {
        self.load().machines()
    }

    pub fn save(&self, machines: &[Machine]) -> bool {
        if self.read_only {
            return false;
        }
        let Some(text) = serialize(machines) else {
            return false;
        };
        self.import_old();
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

pub fn write_lock() -> MutexGuard<'static, ()> {
    WRITES.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn pretty_sorted(value: Value) -> Vec<u8> {
    let mut data = serde_json::to_vec_pretty(&sort_keys(value)).unwrap_or_else(|_| b"{}".to_vec());
    data.push(b'\n');
    data
}

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
