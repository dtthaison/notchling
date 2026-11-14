use super::{start, LogStart};
use crate::layout::Layout;
use crate::limits_ops_tests::TempDir;
use crate::log_activity::Publish;
use crate::log_bus::wall_ms;
use notchling_engine::activity::{DbQuery, EventsDb, Opened};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn layout(tmp: &TempDir) -> Layout {
    std::fs::create_dir_all(tmp.path("notchling")).unwrap();
    Layout::resolve(None, Some(tmp.root().as_os_str()), || None).unwrap()
}

fn quiet() -> Publish {
    Arc::new(|_, _, _| {})
}

fn count(db: &Arc<Mutex<EventsDb>>, kind: &str) -> usize {
    db.lock()
        .unwrap()
        .list(&DbQuery {
            kinds: vec![kind.to_owned()],
            limit: 1000,
            ..DbQuery::default()
        })
        .unwrap()
        .len()
}

fn stop(log: &LogStart, started: usize) {
    let began = std::time::Instant::now();
    while count(&log.db, "daemon.started") < started && began.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    log.bus.close(Duration::from_secs(5));
}

fn legacy_audit() -> String {
    let now = wall_ms();
    format!(
        "{{\"time_ms\":{},\"op\":\"session.reply\",\"tier\":\"app\",\"peer\":\"/Applications/Notchling.app/Contents/MacOS/notchling-app\",\"result\":\"ok\"}}\n{{\"time_ms\":{},\"op\":\"vault.pending\",\"tier\":\"agent\",\"peer\":\"/usr/bin/curl\",\"result\":\"forbidden\"}}\n",
        now - 5_000,
        now - 4_000
    )
}

#[test]
fn first_start_moves_legacy_and_imports_once() {
    let tmp = TempDir::new("start-first");
    let layout = layout(&tmp);
    std::fs::write(layout.audit_path(), legacy_audit()).unwrap();
    let log = start(&layout, "0.1.0", quiet());
    stop(&log, 1);
    assert!(!layout.audit_path().exists());
    assert!(layout
        .audit_dir()
        .join("legacy")
        .join("audit.jsonl")
        .is_file());
    assert_eq!(count(&log.db, "op.called"), 1);
    assert_eq!(count(&log.db, "api.refused"), 1);
    assert_eq!(count(&log.db, "daemon.started"), 1);
    assert_eq!(
        log.db.lock().unwrap().meta_get("imported_audit").as_deref(),
        Some("1")
    );
    assert!(layout.salt_path().is_file());
    assert!(layout.host_id_path().is_file());
}

#[test]
fn second_start_imports_nothing() {
    let tmp = TempDir::new("start-second");
    let layout = layout(&tmp);
    std::fs::write(layout.audit_path(), legacy_audit()).unwrap();
    let first = start(&layout, "0.1.0", quiet());
    stop(&first, 1);
    let host = first.host.clone();
    drop(first);
    let second = start(&layout, "0.1.0", quiet());
    stop(&second, 2);
    assert_eq!(second.host, host);
    assert_eq!(count(&second.db, "op.called"), 1);
    assert_eq!(count(&second.db, "api.refused"), 1);
    assert_eq!(count(&second.db, "daemon.started"), 2);
}

#[test]
fn damaged_db_rebuilds_from_months() {
    let tmp = TempDir::new("start-rebuild");
    let layout = layout(&tmp);
    let first = start(&layout, "0.1.0", quiet());
    stop(&first, 1);
    assert_eq!(count(&first.db, "daemon.started"), 1);
    drop(first);
    std::fs::write(layout.events_db_path(), b"this is not a database at all").unwrap();
    let second = start(&layout, "0.1.0", quiet());
    stop(&second, 2);
    assert_eq!(second.db.lock().unwrap().opened(), Opened::Rebuilt);
    assert_eq!(count(&second.db, "daemon.started"), 2);
}

#[test]
fn salt_replaced_pushes_salt_rotated() {
    let tmp = TempDir::new("start-salt");
    let layout = layout(&tmp);
    let first = start(&layout, "0.1.0", quiet());
    stop(&first, 1);
    assert_eq!(count(&first.db, "log.salt_rotated"), 0);
    drop(first);
    std::fs::write(layout.salt_path(), b"short").unwrap();
    let second = start(&layout, "0.1.0", quiet());
    stop(&second, 2);
    assert_eq!(count(&second.db, "log.salt_rotated"), 1);
}

#[test]
fn a_host_id_change_is_pushed_once() {
    let tmp = TempDir::new("start-host");
    let layout = layout(&tmp);
    let first = start(&layout, "0.1.0", quiet());
    stop(&first, 1);
    assert_eq!(count(&first.db, "host.identity"), 0);
    drop(first);
    let bridge = layout.bridge_dir();
    std::fs::create_dir_all(&bridge).unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["Notchling test".to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    std::fs::write(bridge.join("cert.pem"), cert.pem()).unwrap();
    std::fs::write(bridge.join("key.pem"), key.serialize_pem()).unwrap();
    let second = start(&layout, "0.1.0", quiet());
    stop(&second, 2);
    assert_eq!(count(&second.db, "host.identity"), 1);
    assert_ne!(second.host.as_str().len(), 0);
}
