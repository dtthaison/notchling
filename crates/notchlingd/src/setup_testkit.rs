use crate::acting_ops::now_ms;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_ops::ConsentOps;
use crate::consent_testkit::verified;
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::limits_ops_tests::TempDir;
use notchling_engine::consent::ConsentStore;
use notchling_events::{MemorySink, Salt};
use notchling_local_api::{AgentIdentity, ApiError};
use std::sync::{Arc, Mutex};

pub(crate) struct Cards {
    pub _tmp: TempDir,
    pub consent: Arc<ConsentOps>,
    pub who: Arc<Mutex<AgentIdentity>>,
}

impl Drop for Cards {
    fn drop(&mut self) {
        self.consent.close();
    }
}

pub(crate) fn cards(tag: &str) -> Cards {
    let tmp = TempDir::new(&format!("setup-cards-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let consent = ConsentOps::with_clock(
        Arc::new(ConsentStore::in_memory_for_tests()),
        room,
        Some(inbox),
        Some(Events::fixed(Arc::new(MemorySink::new()), Salt([3; 32]))),
        Arc::new(now_ms),
    );
    let who = Arc::new(Mutex::new(verified("parent")));
    let seen = who.clone();
    consent.bind_identity(Arc::new(move |_| seen.lock().unwrap().clone()));
    consent.start();
    Cards {
        _tmp: tmp,
        consent,
        who,
    }
}

pub(crate) fn reason_of(e: &ApiError) -> String {
    e.data
        .as_ref()
        .and_then(|d| d["reason"].as_str())
        .unwrap_or_default()
        .to_string()
}

pub(crate) type Snapshot = Vec<(std::path::PathBuf, std::time::SystemTime, Vec<u8>)>;

pub(crate) fn snapshot(root: &std::path::Path) -> Snapshot {
    fn walk(dir: &std::path::Path, out: &mut Snapshot) {
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = read.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                let mtime = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                out.push((
                    path.clone(),
                    mtime,
                    std::fs::read(&path).unwrap_or_default(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}
