use crate::host_id;
use crate::layout::Layout;
use crate::log_activity::{ActivitySink, Publish};
use crate::log_audit::{AuditSink, SharedAudit, StartInfo};
use crate::log_bus::{wall_ms, BusConfig, Enabled, EnvMaker, LogBus};
use crate::log_checks::{Checker, Warnings};
use notchling_engine::activity::{import_once, rebuild_from_months, EventsDb, ImportFiles, Opened};
use notchling_engine::logs::{move_legacy, AuditDir, KeyStore, LegacyFile};
use notchling_events::ids::{new_boot, new_op_id, random_bytes};
use notchling_events::kinds::choices::SinkName;
use notchling_events::kinds::diagnostics::HostIdentity;
use notchling_events::kinds::security::DataRoot;
use notchling_events::kinds::security_log::{DropReason, LogDropped, LogSaltRotated};
use notchling_events::salt::{load_or_create, SaltChange};
use notchling_events::spec::{DefaultState, Outcome};
use notchling_events::{Count, Emit, Event, Hex, Salt, Version};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const KEY_WAIT: Duration = Duration::from_secs(2);

pub struct LogStart {
    pub bus: Arc<LogBus>,
    pub db: Arc<Mutex<EventsDb>>,
    pub salt: Salt,
    pub host: Hex,
    pub audit: Arc<Mutex<AuditSink>>,
    pub audit_root: PathBuf,
    pub keys: Arc<dyn KeyStore>,
    health: Arc<OnceLock<Arc<Checker>>>,
}

impl LogStart {
    pub fn checker(&self, warnings: Arc<dyn Warnings>, publish: Publish) -> Arc<Checker> {
        let checker = Checker::new(
            AuditDir::new(self.audit_root.clone()),
            self.keys.clone(),
            self.audit.clone(),
            self.bus.clone(),
            publish,
            warnings,
            Arc::new(wall_ms),
        );
        let _ = self.health.set(checker.clone());
        checker
    }
}

fn legacy_files(dir: &AuditDir) -> Vec<PathBuf> {
    let legacy = dir.legacy_dir();
    let mut names: Vec<String> = std::fs::read_dir(&legacy)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| name.starts_with("audit.jsonl"))
                .collect()
        })
        .unwrap_or_default();
    names.sort_by_key(|name| (!name.starts_with("audit.jsonl."), name.clone()));
    names.into_iter().map(|name| legacy.join(name)).collect()
}

fn open_db(path: &Path, failed: &mut Vec<SinkName>) -> EventsDb {
    match EventsDb::open(path) {
        Ok(db) => db,
        Err(_) => {
            failed.push(SinkName::Activity);
            EventsDb::memory().unwrap_or_else(|e| panic!("no in-memory database: {e}"))
        }
    }
}

fn fill_db(db: &mut EventsDb, dir: &AuditDir, layout: &Layout, host: &Hex) {
    if db.opened() != Opened::Existing {
        let months: Vec<(u32, String)> = dir
            .months()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| dir.read(m).ok().map(|text| (m, text)))
            .collect();
        let rebuilt = rebuild_from_months(&months, host.as_str()).rows;
        if !rebuilt.is_empty() {
            let _ = db.insert(&rebuilt);
        }
    }
    let files = ImportFiles {
        audit: legacy_files(dir),
        remote_log: layout.remote_log_path(),
        account_log: layout.account_log_path(),
    };
    let _ = import_once(db, &files, host.as_str());
}

fn macos_version() -> Version {
    Version::new(&notchling_platform::os_version().unwrap_or_else(|| "0".to_owned()))
}

fn first_events(
    salt_change: &SaltChange,
    salt: &Salt,
    identity: Option<(Hex, Hex)>,
    failed: &[SinkName],
) -> Vec<Event> {
    let now = wall_ms();
    let mut events = Vec::new();
    if let SaltChange::Replaced { old_id } = salt_change {
        events.push(Event::new(
            &LogSaltRotated {
                old_id: old_id.clone(),
                new_id: salt.id(),
            },
            Outcome::Info,
            now,
        ));
    }
    if let Some((old, new)) = identity {
        events.push(Event::new(&HostIdentity { old, new }, Outcome::Info, now));
    }
    for sink in failed {
        events.push(Event::new(
            &LogDropped {
                sink: *sink,
                count: Count(1),
                reason: DropReason::WriteFailed,
            },
            Outcome::Info,
            now,
        ));
    }
    events
}

pub fn start(layout: &Layout, version: &str, publish: Publish) -> LogStart {
    let mut failed: Vec<SinkName> = Vec::new();
    let _ = notchling_platform::files::ensure_private_dir(&layout.state_dir().join("daemon"));
    let dir = AuditDir::new(layout.audit_dir());
    if dir.ensure().is_err() {
        failed.push(SinkName::Audit);
    }
    let moved: Vec<LegacyFile> = move_legacy(&layout.state_dir(), &dir).unwrap_or_else(|_| {
        failed.push(SinkName::Audit);
        Vec::new()
    });
    let (salt, salt_change) = load_or_create(&layout.salt_path()).unwrap_or_else(|_| {
        (
            Salt(random_bytes::<32>().unwrap_or([0; 32])),
            SaltChange::Same,
        )
    });
    let (host, identity) = host_id::resolve(&layout.bridge_dir(), &layout.host_id_path())
        .unwrap_or_else(|_| (new_op_id(), None));
    let mut db = open_db(&layout.events_db_path(), &mut failed);
    fill_db(&mut db, &dir, layout, &host);
    let env = EnvMaker::new(
        host.clone(),
        new_boot(),
        std::process::id(),
        db.max_seq() + 1,
    );
    let keys = layout.key_store();
    let info = StartInfo {
        daemon: Version::new(version),
        macos: macos_version(),
        data_root: if layout.fixture {
            DataRoot::Fixture
        } else {
            DataRoot::Real
        },
        key_wait: KEY_WAIT,
        env: env.clone(),
        clock: Arc::new(wall_ms),
    };
    let audit = Arc::new(Mutex::new(AuditSink::new(
        AuditDir::new(layout.audit_dir()),
        keys.clone(),
        info,
        moved,
        salt,
    )));
    let db = Arc::new(Mutex::new(db));
    let activity = ActivitySink::new(db.clone(), salt, publish);
    let health: Arc<OnceLock<Arc<Checker>>> = Arc::new(OnceLock::new());
    let mut cfg = BusConfig::daemon(host.clone());
    cfg.env = env;
    let late = health.clone();
    cfg.on_health = Some(Arc::new(move |ok| {
        if let Some(checker) = late.get() {
            checker.write_health(ok);
        }
    }));
    let enabled: Enabled = Arc::new(|spec| spec.default == DefaultState::On);
    let bus = LogBus::start(
        cfg,
        vec![Box::new(SharedAudit(audit.clone())), Box::new(activity)],
        enabled,
    );
    for event in first_events(&salt_change, &salt, identity, &failed) {
        bus.push(event);
    }
    LogStart {
        bus,
        db,
        salt,
        host,
        audit,
        audit_root: layout.audit_dir(),
        keys,
        health,
    }
}

#[cfg(test)]
#[path = "log_start_tests.rs"]
mod tests;
