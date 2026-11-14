use crate::log_activity::Publish;
use crate::log_audit::AuditSink;
use crate::log_audit_tests::{called, refused, rig, with_history, Rig, T0};
use crate::log_bus::Sink;
use crate::log_checks::{Checker, Warnings};
use notchling_engine::logs::KeyStore;
use notchling_events::kinds::security_log::ChainReason;
use notchling_events::MemorySink;
use serde_json::Value;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct FakeWarnings {
    pub(crate) raised: Mutex<Vec<(String, String)>>,
    pub(crate) resolved: Mutex<Vec<String>>,
}

impl Warnings for FakeWarnings {
    fn raise(&self, id: &str, title: &str, _detail: &str) {
        self.raised
            .lock()
            .unwrap()
            .push((id.to_owned(), title.to_owned()));
    }

    fn resolve(&self, id: &str) {
        self.resolved.lock().unwrap().push(id.to_owned());
    }
}

pub(crate) struct Wired {
    pub(crate) rig: Rig,
    pub(crate) audit: Arc<Mutex<AuditSink>>,
    pub(crate) events: Arc<MemorySink>,
    pub(crate) warnings: Arc<FakeWarnings>,
    pub(crate) published: Arc<Mutex<Vec<Value>>>,
    pub(crate) checker: Arc<Checker>,
}

pub(crate) fn wire(rig: Rig) -> Wired {
    let audit = Arc::new(Mutex::new(rig.sink()));
    let events = Arc::new(MemorySink::new());
    let warnings = Arc::new(FakeWarnings::default());
    let published = Arc::new(Mutex::new(Vec::new()));
    let seen = published.clone();
    let publish: Publish = Arc::new(move |topic, event, data| {
        assert_eq!((topic, event), ("log", "log.check"));
        seen.lock().unwrap().push(data);
    });
    let clock = rig.clock.clone();
    let checker = Checker::new(
        rig.dir(),
        rig.keys.clone(),
        audit.clone(),
        events.clone(),
        publish,
        warnings.clone(),
        Arc::new(move || clock.load(Ordering::SeqCst)),
    );
    Wired {
        rig,
        audit,
        events,
        warnings,
        published,
        checker,
    }
}

impl Wired {
    pub(crate) fn write(&self, events: Vec<notchling_events::Event>) {
        let batch: Vec<_> = events.into_iter().map(|e| self.rig.stamp(e)).collect();
        let audit_n = vec![None; batch.len()];
        self.audit.lock().unwrap().write(&batch, &audit_n);
    }

    fn edit_month(&self, index: usize, from: &str, to: &str) {
        let dir = self.rig.dir();
        let month = dir.months().unwrap()[index];
        let text = dir.read(month).unwrap();
        assert!(text.contains(from));
        std::fs::write(dir.path_of(month), text.replacen(from, to, 1)).unwrap();
    }
}

#[test]
fn start_check_is_intact_for_a_clean_chain() {
    let wired = wire(rig("checks-clean"));
    wired.write(vec![called(T0), called(T0 + 1)]);
    let report = wired.checker.run(true);
    assert_eq!(report.result, "intact");
    assert_eq!(report.sealing, "sealed");
    assert!(report.chain_started.is_some());
    assert!(wired.warnings.raised.lock().unwrap().is_empty());
    assert_eq!(wired.published.lock().unwrap().len(), 1);
    assert!(wired.events.names().contains(&"audit.checked"));
}

#[test]
fn file_ahead_of_checkpoint_is_intact() {
    let wired = wire(rig("checks-ahead"));
    wired.write(vec![refused(T0)]);
    wired.audit.lock().unwrap().flush(true).unwrap();
    let head = KeyStore::read_head(&*wired.rig.keys).unwrap();
    wired.write(vec![refused(T0 + 20_000), called(T0 + 20_001)]);
    let report = wired.checker.run(true);
    assert_eq!(report.result, "intact");
    assert_eq!(report.checkpoint_n, Some(head.n));
    assert!(report.to_n > head.n);
}

#[test]
fn edited_month_raises_inbox_item_and_publishes_check() {
    let wired = wire(rig("checks-edited"));
    wired.write(vec![called(T0), called(T0 + 1)]);
    wired.edit_month(0, "\"ms\":2", "\"ms\":3");
    let report = wired.checker.run(true);
    assert_eq!(report.result, "broken");
    assert!(report.first_bad_n.is_some());
    let raised = wired.warnings.raised.lock().unwrap().clone();
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].1, "Audit Log Changed Outside Notchling");
    assert!(raised[0].0.starts_with("log-warning:"));
    let published = wired.published.lock().unwrap().clone();
    assert_eq!(published.last().unwrap()["result"], "broken");
    wired.checker.run(true);
    assert_eq!(wired.warnings.raised.lock().unwrap().len(), 1);
}

#[test]
fn unsealed_raises_item_once() {
    let rig = rig("checks-unsealed");
    with_history(&rig);
    let wired = wire(rig);
    wired.write(vec![called(T0 + 1)]);
    let first = wired.checker.run(true);
    assert_eq!(first.result, "unsealed");
    assert_eq!(first.sealing, "unsealed");
    assert_eq!(first.why, Some("key_missing"));
    wired.checker.run(true);
    let raised = wired.warnings.raised.lock().unwrap().clone();
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].1, "Audit Log Not Sealed");
}

#[test]
fn item_resolves_when_intact_again() {
    let rig = rig("checks-resolve");
    with_history(&rig);
    let wired = wire(rig);
    wired.write(vec![called(T0 + 1)]);
    wired.checker.run(true);
    let id = wired.warnings.raised.lock().unwrap()[0].0.clone();
    wired
        .audit
        .lock()
        .unwrap()
        .start_new_chain(ChainReason::User)
        .unwrap();
    let report = wired.checker.run(true);
    assert_eq!(report.result, "intact");
    assert_eq!(wired.warnings.resolved.lock().unwrap().clone(), vec![id]);
}

#[test]
fn full_check_reads_every_month() {
    let wired = wire(rig("checks-full"));
    let later = T0 + 40 * 86_400_000;
    wired.write(vec![called(T0), called(later)]);
    assert_eq!(wired.checker.run(true).result, "intact");
    wired.edit_month(0, "\"ms\":2", "\"ms\":3");
    assert_eq!(
        wired.checker.run(false).result,
        "intact",
        "an incremental check starts after what it already checked"
    );
    assert_eq!(wired.checker.run(true).result, "broken");
}

#[test]
fn a_write_failure_raises_one_item_and_clears_when_writes_work() {
    let wired = wire(rig("checks-write"));
    wired.checker.write_health(false);
    wired.checker.write_health(false);
    let raised = wired.warnings.raised.lock().unwrap().clone();
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].1, "Audit Log Can't Be Written");
    wired.checker.write_health(true);
    assert_eq!(wired.warnings.resolved.lock().unwrap().len(), 1);
}

#[test]
fn the_inbox_item_opens_settings_logs_and_is_withdrawn_when_fixed() {
    use crate::acting_ops_tests::FakeRoom;
    use crate::inbox_ops::InboxOps;
    use crate::log_checks::InboxWarnings;
    use notchling_engine::inbox::State;
    let tmp = crate::limits_ops_tests::TempDir::new("checks-inbox");
    let inbox = InboxOps::new(tmp.path("data"), Arc::new(FakeRoom::default()));
    let warnings = InboxWarnings::new(inbox.clone(), Arc::new(|| T0));
    warnings.raise("log-warning:1", "Audit Log Not Sealed", "detail");
    let items = inbox.items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state, State::Open);
    assert_eq!(items[0].open, Some(serde_json::json!({"settings": "logs"})));
    warnings.resolve("log-warning:1");
    assert_eq!(inbox.items()[0].state, State::Withdrawn);
}
