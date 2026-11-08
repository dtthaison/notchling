//! Fakes for the machines tests (Swift `MachineSetupFakes`). Nothing here starts a process or opens
//! a socket; every secret is an obviously fake value.

use super::github::{allows, GitHubHttp, HttpAnswer};
use super::masks::Masks;
use super::registry::{Machine, MachineAbilities, MachineOs, MachineRepo, SdWorkerSettings};
use super::runner::{Channel, OutputSink, ScriptedRunner};
use super::shell;
use super::step::{FixContext, MacFacts, Opener, SecretSource, StepContext};
use crate::accounts::Secret;
use crate::vault::{VaultEntry, VaultRequest};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A vault of fixed values.
#[derive(Clone, Default)]
pub struct FakeSecrets {
    pub values: BTreeMap<String, String>,
    pub entries: BTreeMap<String, VaultEntry>,
}

impl FakeSecrets {
    pub fn new() -> FakeSecrets {
        FakeSecrets::default()
    }

    pub fn with(pairs: &[(&str, &str)]) -> FakeSecrets {
        FakeSecrets {
            values: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            entries: BTreeMap::new(),
        }
    }
}

impl SecretSource for FakeSecrets {
    fn value(&self, name: &str) -> Option<Secret> {
        self.values.get(name).map(|v| Secret::new(v.as_str()))
    }
    fn entry(&self, name: &str) -> Option<VaultEntry> {
        self.entries.get(name).cloned()
    }
}

/// Answers by path plus query (`/user/repos?page=2`), 404 for anything not listed, and records
/// every request with its bearer, including the ones the real client would refuse (those get
/// status 0). With a delay it also counts the most requests in flight at once.
#[derive(Default)]
pub struct PathHttp {
    answers: BTreeMap<String, HttpAnswer>,
    seen: Mutex<Vec<(String, String)>>,
    delay: Option<Duration>,
    in_flight: AtomicUsize,
    peak: AtomicUsize,
}

impl PathHttp {
    pub fn new(answers: BTreeMap<String, HttpAnswer>) -> PathHttp {
        PathHttp {
            answers,
            ..PathHttp::default()
        }
    }

    /// Every request waits `delay` and is answered 404.
    pub fn slow(delay: Duration) -> PathHttp {
        PathHttp {
            delay: Some(delay),
            ..PathHttp::default()
        }
    }

    /// (url, bearer) of every request.
    pub fn requests(&self) -> Vec<(String, String)> {
        lock(&self.seen).clone()
    }

    pub fn urls(&self) -> Vec<String> {
        self.requests().into_iter().map(|(u, _)| u).collect()
    }

    pub fn max_in_flight(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

impl GitHubHttp for PathHttp {
    fn get(&self, url: &str, bearer: &Secret) -> HttpAnswer {
        lock(&self.seen).push((url.into(), bearer.expose().into()));
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        if let Some(delay) = self.delay {
            std::thread::sleep(delay);
        }
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        match allows(url) {
            None => HttpAnswer::none(),
            Some(path) => self
                .answers
                .get(path)
                .cloned()
                .unwrap_or_else(|| HttpAnswer::new(404, &[], "")),
        }
    }
}

/// Collects the chunks a Fix logs, with their pipe.
#[derive(Default)]
pub struct LogBox {
    chunks: Mutex<Vec<(Channel, String)>>,
}

impl LogBox {
    pub fn sink(self: &Arc<Self>) -> Arc<OutputSink> {
        let me = self.clone();
        Arc::new(move |c, t: &str| lock(&me.chunks).push((c, t.to_string())))
    }

    pub fn joined(&self) -> String {
        lock(&self.chunks).iter().map(|(_, t)| t.as_str()).collect()
    }
}

/// Records what a Fix asked this Mac to open.
#[derive(Default)]
pub struct RecordingOpener {
    pub pages: Mutex<Vec<String>>,
    pub forms: Mutex<Vec<VaultRequest>>,
}

impl Opener for RecordingOpener {
    fn open_page(&self, url: &str) {
        lock(&self.pages).push(url.into());
    }
    fn open_form(&self, request: &VaultRequest) {
        lock(&self.forms).push(request.clone());
    }
}

impl RecordingOpener {
    pub fn pages(&self) -> Vec<String> {
        lock(&self.pages).clone()
    }
    pub fn forms(&self) -> Vec<VaultRequest> {
        lock(&self.forms).clone()
    }
}

pub const REPO: &str = "dtthaison/lingeva-mono";

pub struct TestMachines;

impl TestMachines {
    fn machine(
        name: &str,
        user: &str,
        home: &str,
        os: MachineOs,
        python: &str,
        kind: &str,
    ) -> Machine {
        let mut m = Machine::new(name, name, user);
        m.os = Some(os);
        m.abilities = Some(MachineAbilities {
            home: Some(home.into()),
            ..MachineAbilities::default()
        });
        let mut job = SdWorkerSettings::new(name);
        job.python = python.into();
        job.sd_path = shell::sd_py(home);
        job.repos = vec![MachineRepo {
            github: REPO.into(),
            local_path: "/Users/sondoan/go/src/pp/lingeva-mono".into(),
            check_command: "true".into(),
            machine_path: None,
            verify_skill: None,
        }];
        job.kinds = vec![kind.into()];
        m.set_sd_worker(Some(job));
        m
    }

    pub fn mac(name: &str) -> Machine {
        Self::machine(
            name,
            "sondoan",
            "/Users/sondoan",
            MachineOs::MacOs,
            "/opt/homebrew/bin/python3",
            "ios",
        )
    }

    pub fn linux(name: &str) -> Machine {
        Self::machine(
            name,
            "agent",
            "/home/agent",
            MachineOs::Linux,
            "/usr/bin/python3",
            "backend",
        )
    }
}

/// Builds the contexts a step is called with.
pub struct Ctx {
    pub machine: Machine,
    pub all: Option<Vec<Machine>>,
    pub runner: Arc<ScriptedRunner>,
    pub secrets: Arc<FakeSecrets>,
    pub http: Arc<dyn GitHubHttp>,
    pub opener: Arc<RecordingOpener>,
    pub masks: Masks,
    pub now: f64,
}

impl Ctx {
    pub fn new(machine: Machine) -> Ctx {
        Ctx {
            machine,
            all: None,
            runner: Arc::new(ScriptedRunner::new()),
            secrets: Arc::new(FakeSecrets::new()),
            http: Arc::new(PathHttp::default()),
            opener: Arc::new(RecordingOpener::default()),
            masks: Masks::new(),
            now: 1_800_000_000.0,
        }
    }

    pub fn mac() -> Ctx {
        Ctx::new(TestMachines::mac("m1"))
    }

    pub fn all(mut self, all: Vec<Machine>) -> Ctx {
        self.all = Some(all);
        self
    }

    pub fn runner(mut self, runner: &Arc<ScriptedRunner>) -> Ctx {
        self.runner = runner.clone();
        self
    }

    pub fn secrets(mut self, pairs: &[(&str, &str)]) -> Ctx {
        self.secrets = Arc::new(FakeSecrets::with(pairs));
        self
    }

    pub fn http(mut self, http: Arc<dyn GitHubHttp>) -> Ctx {
        self.http = http;
        self
    }

    /// Hides `value` as `[secret:<name>]` the way the engine's masks do.
    pub fn hiding(mut self, name: &str, value: &str) -> Ctx {
        self.masks.hide(name, value);
        self
    }

    pub fn step(&self) -> StepContext {
        StepContext {
            machine: self.machine.clone(),
            all: self
                .all
                .clone()
                .unwrap_or_else(|| vec![self.machine.clone()]),
            runner: self.runner.clone(),
            secrets: self.secrets.clone(),
            http: self.http.clone(),
            opener: self.opener.clone(),
            mac: MacFacts::default(),
            now: self.now,
            scratch: PathBuf::from("/nonexistent/scratch-for-tests"),
            masks: self.masks.clone(),
        }
    }

    /// A Fix's context; `admin` is its admin password, which the Fix's masks hide as well.
    pub fn fix(&self, admin: Option<&str>) -> (FixContext, Arc<LogBox>) {
        let log = Arc::new(LogBox::default());
        let ctx = FixContext {
            step: self.step(),
            admin_password: admin.map(Secret::new),
            log: log.sink(),
            masks: self.masks.clone().with_admin(admin),
        };
        (ctx, log)
    }
}

/// Panics (the test fails) when an 8-character piece of `value` shows in `text`.
pub fn assert_no_fragment(value: &str, text: &str) {
    let chars: Vec<char> = value.chars().collect();
    for i in 0..=(chars.len() - 8) {
        let piece: String = chars[i..i + 8].iter().collect();
        assert!(
            !text.contains(&piece),
            "a fragment of the value shows at {i}: {text}"
        );
    }
}

/// A 108-character token like a Claude OAuth token (fake).
pub fn long_token() -> String {
    let alphabet: Vec<char> = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789-_"
        .chars()
        .collect();
    let body: String = (0..95)
        .map(|i| alphabet[(i * 7 + i / 3) % alphabet.len()])
        .collect();
    format!("sk-ant-oat01-{body}")
}

/// A folder removed when it goes out of scope.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "notchling-machines-{tag}-{}-{n}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The output of `xcrun simctl` the Xcode probe reads: three iPhones and an iPad under iOS, an
/// Apple TV under tvOS.
pub const SIMCTL_OK: &str = r#"Xcode 26.1
@@runtimes
iOS 26.5 (26.5 - 23F77) - com.apple.CoreSimulator.SimRuntime.iOS-26-5
@@devices
{
  "devices" : {
    "com.apple.CoreSimulator.SimRuntime.tvOS-26-5" : [
      {
        "isAvailable" : true,
        "state" : "Shutdown",
        "name" : "Apple TV 4K (3rd generation)",
        "udid" : "0A1B2C3D-0000-4000-8000-000000000001"
      }
    ],
    "com.apple.CoreSimulator.SimRuntime.iOS-26-5" : [
      {
        "isAvailable" : true,
        "state" : "Shutdown",
        "name" : "iPhone 17",
        "udid" : "0A1B2C3D-0000-4000-8000-000000000002"
      },
      {
        "isAvailable" : true,
        "state" : "Shutdown",
        "name" : "iPhone 17 Pro",
        "udid" : "0A1B2C3D-0000-4000-8000-000000000003"
      },
      {
        "isAvailable" : true,
        "state" : "Shutdown",
        "name" : "iPhone 17 Pro Max",
        "udid" : "0A1B2C3D-0000-4000-8000-000000000004"
      },
      {
        "isAvailable" : true,
        "state" : "Shutdown",
        "name" : "iPad Pro 13-inch (M5)",
        "udid" : "0A1B2C3D-0000-4000-8000-000000000005"
      }
    ]
  }
}
"#;
