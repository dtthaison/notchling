use crate::inbox_ops::InboxOps;
use notchling_engine::inbox::{Action, ActionId, Item, Kind, Source, State};
use notchling_engine::machines::versions_facts;
use notchling_engine::store;
use notchling_engine::vault::random_id;
use notchling_engine::versions::drift::{drift, plan_notices, Drift, Key};
use notchling_engine::versions::record::{Fact, Recorder};
use notchling_engine::versions::selfupdate_log;
use notchling_engine::versions::{Tool, Version, WorkerTools};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const NOTICED_DOC: &str = "versions/drift-noticed";

pub struct Drifts {
    data: PathBuf,
    recorder: Arc<dyn Recorder>,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    inbox: Mutex<Option<Arc<InboxOps>>>,
    ids: Mutex<BTreeMap<Key, String>>,
    gate: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn read_workers(data: &std::path::Path) -> BTreeMap<String, WorkerTools> {
    let saved = store::doc(data, versions_facts::DOC)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str::<Map<String, Value>>(&text).ok())
        .unwrap_or_default();
    saved
        .into_iter()
        .filter_map(|(machine, value)| Some((machine, serde_json::from_value(value).ok()?)))
        .collect()
}

fn read_noticed(data: &std::path::Path) -> BTreeSet<Key> {
    let Some(text) = store::doc(data, NOTICED_DOC).ok().flatten() else {
        return BTreeSet::new();
    };
    let rows: Vec<(String, String, String)> = serde_json::from_str(&text).unwrap_or_default();
    rows.into_iter()
        .filter_map(|(machine, tool, found)| {
            let found = Version::parse(&found)?;
            Some((machine, Tool::from_name(&tool)?, found.as_str().to_string()))
        })
        .collect()
}

fn write_noticed(data: &std::path::Path, noticed: &BTreeSet<Key>) -> Result<(), String> {
    let rows: Vec<(&str, &str, &str)> = noticed
        .iter()
        .map(|(machine, tool, found)| (machine.as_str(), tool.name(), found.as_str()))
        .collect();
    store::put_doc(
        data,
        NOTICED_DOC,
        &serde_json::to_string(&rows).map_err(|e| e.to_string())?,
    )
}

impl Drifts {
    pub fn new(
        data: PathBuf,
        recorder: Arc<dyn Recorder>,
        clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    ) -> Drifts {
        Drifts {
            data,
            recorder,
            clock,
            inbox: Mutex::new(None),
            ids: Mutex::new(BTreeMap::new()),
            gate: Mutex::new(()),
        }
    }

    pub fn set_inbox(&self, inbox: Arc<InboxOps>) {
        *lock(&self.inbox) = Some(inbox);
    }

    pub fn pass(&self, reference: &BTreeMap<Tool, Version>) {
        let _gate = lock(&self.gate);
        self.replay_self_updates();
        let Some(inbox) = lock(&self.inbox).clone() else {
            return;
        };
        let workers = read_workers(&self.data);
        let now = drift(reference, &workers, &BTreeSet::new(), &BTreeSet::new());
        let plan = plan_notices(&now, &read_noticed(&self.data));
        for key in &plan.withdraw {
            if let Some(id) = lock(&self.ids).remove(key) {
                inbox.resolve(&id, State::Withdrawn, None, None);
            }
        }
        if write_noticed(&self.data, &plan.noticed).is_err() {
            return;
        }
        for found in &plan.raise {
            self.raise(&inbox, found);
        }
    }

    fn raise(&self, inbox: &InboxOps, found: &Drift) {
        self.recorder.record(Fact::Drift {
            tool: found.tool,
            machine: found.machine.clone(),
            expected: found.expected.clone(),
            found: found.found.clone(),
        });
        let Some(id) = random_id("version-drift:") else {
            return;
        };
        inbox.raise(
            Item {
                id: id.clone(),
                kind: Kind::VersionDrift,
                title: found.title(),
                detail: found.detail(),
                source: Source::default(),
                created_ms: (self.clock)(),
                expires_ms: None,
                actions: vec![Action {
                    id: ActionId::Open,
                    label: "Open Machines".to_string(),
                    primary: true,
                }],
                state: State::Open,
                answer: None,
                answered_by: None,
                seen: false,
                open: Some(json!({ "machines": found.machine })),
                closed_ms: None,
            },
            None,
        );
        lock(&self.ids).insert(found.key(), id);
    }

    fn replay_self_updates(&self) {
        let entries = selfupdate_log::unrecorded(&self.data);
        let Some(last) = entries.last().map(|e| e.seq) else {
            return;
        };
        for entry in &entries {
            self.recorder.record(entry.fact());
        }
        let _ = selfupdate_log::mark_recorded(&self.data, last);
    }
}
