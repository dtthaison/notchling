use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use super::addresses::{MachineAddress, Route};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notchling_app: Option<bool>,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeChoices {
    #[serde(default)]
    pub can_be_master: bool,
    #[serde(default)]
    pub acting: bool,
}

impl NodeChoices {
    pub fn is_default(&self) -> bool {
        *self == NodeChoices::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSettings {
    #[serde(default, skip_serializing_if = "NodeChoices::is_default")]
    pub choices: NodeChoices,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    admin_secret: Option<String>,
    #[serde(default = "claude_worker")]
    pub claude_secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addresses: Vec<MachineAddress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abilities: Option<MachineAbilities>,
    #[serde(default)]
    pub jobs: Vec<Job>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeSettings>,
    #[serde(skip)]
    pub route: Option<Route>,
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
            admin_secret: None,
            claude_secret: claude_worker(),
            organization: None,
            addresses: Vec::new(),
            abilities: None,
            jobs: Vec::new(),
            node: None,
            route: None,
        }
    }

    pub fn address_list(&self) -> Vec<MachineAddress> {
        if self.addresses.is_empty() {
            vec![MachineAddress::legacy(&self.host)]
        } else {
            self.addresses.clone()
        }
    }

    pub fn set_addresses(&mut self, list: Vec<MachineAddress>) {
        self.addresses = list.into_iter().map(MachineAddress::normalized).collect();
        self.sync_host();
    }

    pub fn has_addresses(&self) -> bool {
        !self.addresses.is_empty()
    }

    pub fn address_user<'a>(&'a self, address: &'a MachineAddress) -> &'a str {
        address.user.as_deref().unwrap_or(&self.user)
    }

    pub fn route(&self) -> Route {
        self.route
            .clone()
            .unwrap_or_else(|| Route::direct(&self.host, &self.user))
    }

    fn sync_host(&mut self) {
        if let Some(first) = self.addresses.first() {
            self.host = first.host.clone();
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

    pub fn organization_id(&self) -> &str {
        self.organization
            .as_deref()
            .map(str::trim)
            .filter(|o| !o.is_empty())
            .unwrap_or(crate::organizations::PERSONAL_ID)
    }

    pub fn admin_secret(&self) -> Option<&str> {
        self.admin_secret.as_deref().filter(|s| !s.is_empty())
    }

    pub fn set_admin_secret(&mut self, entry: Option<&str>) {
        self.admin_secret = entry.filter(|s| !s.is_empty()).map(str::to_owned);
    }

    fn settle(&mut self) {
        self.addresses = std::mem::take(&mut self.addresses)
            .into_iter()
            .map(MachineAddress::normalized)
            .collect();
        self.sync_host();
        self.admin = Some(self.admin());
        if self.admin_secret.as_deref().is_some_and(str::is_empty) {
            self.admin_secret = None;
        }
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

    pub fn runs_app(&self) -> bool {
        !self.is_linux()
            && self
                .abilities
                .as_ref()
                .is_some_and(|a| a.notchling_app == Some(true))
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
struct FileV3 {
    machines: Vec<Machine>,
}

enum Raw {
    Missing,
    Unreadable(String),
    V1(Vec<Machine>, Vec<u8>),
    V2(Vec<Machine>),
    V3(Vec<Machine>),
    V4(Vec<Machine>),
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
            Raw::V2(machines) | Raw::V3(machines) | Raw::V4(machines) => RegistryState::Loaded {
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
                let Some(backup) = write_backup(&original, &self.old_own, V1_BACKUP) else {
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
                Raw::V2(_) | Raw::V3(_) | Raw::V4(_) => Ok(Some(old)),
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
            let raw = match &row {
                Some(old) => parse(old.as_bytes()),
                None => self.read_files(),
            };
            match raw {
                Raw::Missing => Ok(Some(text)),
                Raw::V2(_) | Raw::V3(_) | Raw::V4(_) => {
                    if let Some(old) = &row {
                        self.keep_older(old, &raw, &text)?;
                    }
                    Ok(Some(text))
                }
                Raw::V1(..) | Raw::Unreadable(_) => {
                    Err("the registry is never written over".into())
                }
            }
        })
        .is_ok()
    }

    fn keep_older(&self, original: &str, raw: &Raw, replacement: &str) -> Result<(), String> {
        let (suffix, version) = match raw {
            Raw::V2(_) => (V2_BACKUP, 2),
            Raw::V3(_) if written_version(replacement) == 4 => (V3_BACKUP, 3),
            _ => return Ok(()),
        };
        write_backup(original.as_bytes(), &self.old_own, suffix)
            .map(|_| ())
            .ok_or_else(|| format!("the version {version} registry could not be saved aside"))
    }

    pub fn adopt_organizations(&self) -> Result<usize, String> {
        if self.read_only {
            return Ok(0);
        }
        self.import_old();
        if let Ok(Some(text)) = store::doc(&self.data_dir, REGISTRY_DOC) {
            if let Raw::V3(machines) | Raw::V4(machines) = parse(text.as_bytes()) {
                if machines.iter().all(|m| {
                    m.organization
                        .as_deref()
                        .is_some_and(|o| !o.trim().is_empty())
                }) {
                    return Ok(0);
                }
            }
        }
        let mut stamped = 0;
        store::update_doc(&self.data_dir, REGISTRY_DOC, |row| {
            let Some(old) = row else {
                return Ok(None);
            };
            let raw = parse(old.as_bytes());
            let (mut machines, was_v2) = match &raw {
                Raw::V2(machines) => (machines.clone(), true),
                Raw::V3(machines) | Raw::V4(machines) => (machines.clone(), false),
                Raw::V1(..) | Raw::Missing => return Ok(Some(old)),
                Raw::Unreadable(why) => return Err(why.clone()),
            };
            for machine in machines.iter_mut().filter(|m| {
                !m.organization
                    .as_deref()
                    .is_some_and(|o| !o.trim().is_empty())
            }) {
                machine.organization = Some(crate::organizations::PERSONAL_ID.into());
                stamped += 1;
            }
            if !was_v2 && stamped == 0 {
                return Ok(Some(old));
            }
            let text = serialize(&machines)
                .ok_or_else(|| "the registry could not be written".to_owned())?;
            self.keep_older(&old, &raw, &text)?;
            Ok(Some(text))
        })?;
        Ok(stamped)
    }

    pub fn machine(&self, name: &str) -> Option<Machine> {
        self.all().into_iter().find(|m| m.name == name)
    }
}

fn file_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

const V1_BACKUP: &str = ".v1-backup";
const V2_BACKUP: &str = ".v2-backup";
const V3_BACKUP: &str = ".v3-backup";

pub fn backup_path(file: &Path) -> PathBuf {
    backup_path_with(file, V1_BACKUP)
}

pub fn backup_path_v2(file: &Path) -> PathBuf {
    backup_path_with(file, V2_BACKUP)
}

pub fn backup_path_v3(file: &Path) -> PathBuf {
    backup_path_with(file, V3_BACKUP)
}

fn backup_path_with(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    if suffix != V1_BACKUP {
        return file
            .with_file_name(crate::organizations::backup::DIR)
            .join(name);
    }
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
        2..=4 => match serde_json::from_slice::<FileV3>(data) {
            Ok(f) => {
                let machines = f
                    .machines
                    .into_iter()
                    .map(|mut m| {
                        m.settle();
                        m
                    })
                    .collect();
                match version {
                    2 => Raw::V2(machines),
                    3 => Raw::V3(machines),
                    _ => Raw::V4(machines),
                }
            }
            Err(e) => Raw::Unreadable(e.to_string()),
        },
        other => Raw::Unreadable(format!("version {other} is not supported")),
    }
}

static WRITES: Mutex<()> = Mutex::new(());

pub fn write_lock() -> MutexGuard<'static, ()> {
    WRITES.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn organizations_in(text: &str) -> Option<Vec<Option<String>>> {
    match parse(text.as_bytes()) {
        Raw::V2(machines) | Raw::V3(machines) | Raw::V4(machines) => {
            Some(machines.into_iter().map(|m| m.organization).collect())
        }
        _ => None,
    }
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
    let version = if settled.iter().any(Machine::has_addresses) {
        4
    } else {
        3
    };
    let file = serde_json::json!({"version": version, "machines": list});
    String::from_utf8(pretty_sorted(file)).ok()
}

fn written_version(text: &str) -> i64 {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.get("version").and_then(Value::as_i64))
        .unwrap_or(0)
}

fn write_backup(original: &[u8], file: &Path, suffix: &str) -> Option<PathBuf> {
    let base = backup_path_with(file, suffix);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut candidates = vec![
        base.clone(),
        PathBuf::from(format!("{}-{stamp}", base.display())),
    ];
    candidates.extend((2..=50).map(|n| PathBuf::from(format!("{}-{stamp}-{n}", base.display()))));
    let dir = base.parent()?;
    if Some(dir) == file.parent() {
        std::fs::create_dir_all(dir).ok()?;
    } else {
        notchling_platform::files::ensure_private_dir(dir).ok()?;
    }
    for candidate in candidates {
        if std::fs::symlink_metadata(&candidate).is_ok() {
            continue;
        }
        if notchling_platform::files::write_private_no_follow(&candidate, original).is_err()
            || std::fs::read(&candidate).ok()? != original
        {
            let _ = std::fs::remove_file(&candidate);
            return None;
        }
        return Some(candidate);
    }
    None
}
