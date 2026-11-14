use crate::activity_ops::Names;
use crate::log_bus::{Sink, SinkResult, Ticked};
use crate::log_queue::Stamped;
use notchling_engine::activity::{expires, row_json, ActivityRow, EventsDb};
use notchling_events::field::FieldValue;
use notchling_events::kinds::choices::PruneReason;
use notchling_events::kinds::security_log::{LogPruned, LogSink};
use notchling_events::spec::{Outcome, Sinks};
use notchling_events::{Count, Event, RefMode, Salt, Time};
use notchling_local_api::topics;
use serde_json::Value;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub const PRUNE_EVERY_MS: i64 = 3_600_000;
pub const CAP_BYTES: u64 = 200 * 1024 * 1024;

pub type Publish = Arc<dyn Fn(&str, &str, Value) + Send + Sync>;

pub struct ActivitySink {
    db: Arc<Mutex<EventsDb>>,
    salt: Salt,
    publish: Publish,
    names: Names,
    cap_bytes: u64,
    last_prune_ms: Option<i64>,
}

fn lock(db: &Mutex<EventsDb>) -> MutexGuard<'_, EventsDb> {
    db.lock().unwrap_or_else(PoisonError::into_inner)
}

fn column(event: &Event, name: &str) -> Option<String> {
    match event.fields.get(name)? {
        FieldValue::Ref { id, .. } => Some(id.to_string()),
        _ => None,
    }
}

impl ActivitySink {
    pub fn new(db: Arc<Mutex<EventsDb>>, salt: Salt, publish: Publish) -> ActivitySink {
        ActivitySink {
            db,
            salt,
            publish,
            names: Arc::new(|_, _| None),
            cap_bytes: CAP_BYTES,
            last_prune_ms: None,
        }
    }

    pub fn with_names(mut self, names: Names) -> ActivitySink {
        self.names = names;
        self
    }

    pub fn with_cap(mut self, bytes: u64) -> ActivitySink {
        self.cap_bytes = bytes;
        self
    }

    fn row_of(&self, stamped: &Stamped, audit_n: Option<u64>) -> ActivityRow {
        let event = &stamped.event;
        let env = &stamped.env;
        let mut body = String::new();
        event
            .fields
            .write_json(&mut body, &RefMode::Plain(self.salt));
        let mut row = ActivityRow::blank(env.seq, event.ts, event.spec.name);
        row.kv = event.spec.kv;
        row.grp = event.spec.group.name().to_owned();
        row.outcome = event.outcome.name().to_owned();
        if let Some(actor) = &env.actor {
            row.actor_tier = Some(actor.tier.name().to_owned());
            row.actor_via = actor.via.map(|v| v.name().to_owned());
            row.actor_exe = actor.exe.as_ref().map(|e| e.as_str().to_owned());
            row.actor_session = actor.session.as_ref().map(|s| s.as_str().to_owned());
            row.device = actor.device.as_ref().map(|d| d.as_str().to_owned());
        }
        row.session = column(event, "session");
        row.folder = column(event, "folder");
        row.account = column(event, "account");
        if let Some(device) = column(event, "device") {
            row.device = Some(device);
        }
        row.op_id = env.op_id.as_ref().map(|id| id.as_str().to_owned());
        row.host = env.host.as_str().to_owned();
        row.audit_n = audit_n;
        row.spooled = env.spooled;
        row.f = body;
        row.expires = expires(event.spec.name, event.ts);
        row
    }
}

impl Sink for ActivitySink {
    fn sinks(&self) -> Sinks {
        Sinks::ACTIVITY
    }

    fn write(&mut self, batch: &[Stamped], audit_n: &[Option<u64>]) -> SinkResult {
        let mut result = SinkResult::written(batch.len());
        let rows: Vec<ActivityRow> = batch
            .iter()
            .enumerate()
            .filter(|(_, s)| s.event.spec.sinks.has(Sinks::ACTIVITY))
            .map(|(i, s)| self.row_of(s, audit_n.get(i).copied().flatten()))
            .collect();
        if rows.is_empty() {
            return result;
        }
        if lock(&self.db).insert(&rows).is_err() {
            result.failed = true;
            return result;
        }
        for row in &rows {
            (self.publish)(topics::LOG, topics::LOG_EVENT, row_json(row, &*self.names));
        }
        result
    }

    fn flush(&mut self, _full: bool) -> io::Result<()> {
        Ok(())
    }

    fn tick(&mut self, now_ms: i64) -> Ticked {
        let due = self
            .last_prune_ms
            .is_none_or(|last| now_ms - last >= PRUNE_EVERY_MS);
        let mut ticked = Ticked::default();
        if !due {
            return ticked;
        }
        self.last_prune_ms = Some(now_ms);
        let mut db = lock(&self.db);
        let span = db.expired_span(now_ms);
        if let Ok(entries) = db.prune(now_ms) {
            if entries > 0 {
                let (from, to) = span.unwrap_or((now_ms, now_ms));
                ticked
                    .events
                    .push(pruned(from, to, entries, PruneReason::Retention, now_ms));
            }
        }
        let before = db.oldest_ts();
        if let Ok(entries) = db.trim_to(self.cap_bytes) {
            if entries > 0 {
                let from = before.unwrap_or(now_ms);
                let to = db.oldest_ts().unwrap_or(now_ms);
                ticked
                    .events
                    .push(pruned(from, to, entries, PruneReason::SizeCap, now_ms));
            }
        }
        ticked
    }
}

fn pruned(from: i64, to: i64, entries: u64, reason: PruneReason, now_ms: i64) -> Event {
    Event::new(
        &LogPruned {
            sink: LogSink::Activity,
            from: Time(from),
            to: Time(to),
            entries: Count(entries),
            reason,
        },
        Outcome::Info,
        now_ms,
    )
}
