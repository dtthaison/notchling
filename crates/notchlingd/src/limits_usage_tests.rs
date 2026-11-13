use super::*;
use notchling_engine::limits::sources::{AccountInfo, LaneInfo};
use notchling_engine::limits::usage::{AccountUsage, UsageWindow};
use std::sync::atomic::AtomicU32;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

fn usage(session: f64) -> AccountUsage {
    AccountUsage {
        session: Some(UsageWindow {
            percent: session,
            resets_at: Some(1_800_003_600.0),
        }),
        week: Some(UsageWindow {
            percent: 5.0,
            resets_at: None,
        }),
        fetched: 1_800_000_000.0,
        ..AccountUsage::default()
    }
}

struct Fake {
    forced: AtomicU32,
    hold: Mutex<Option<Receiver<()>>>,
    entered: Mutex<Option<Sender<()>>>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            forced: AtomicU32::new(0),
            hold: Mutex::new(None),
            entered: Mutex::new(None),
        }
    }
}

impl ClaudeSource for Fake {
    fn sync_usage(&self) {
        panic!("a due-only sync was made");
    }
    fn sync_usage_now(&self) {
        self.forced.fetch_add(1, Ordering::SeqCst);
        if let Some(tx) = self.entered.lock().unwrap().take() {
            let _ = tx.send(());
        }
        let hold = self.hold.lock().unwrap().take();
        if let Some(rx) = hold {
            let _ = rx.recv();
        }
    }
    fn accounts(&self) -> Vec<AccountInfo> {
        ["a", "b"]
            .iter()
            .map(|id| AccountInfo {
                id: id.to_string(),
                email: format!("{id}@x.com"),
                needs_login: false,
            })
            .collect()
    }
    fn active_id(&self) -> Option<String> {
        Some("a".into())
    }
    fn usage(&self, id: &str) -> Option<AccountUsage> {
        (id == "a").then(|| usage(42.0))
    }
    fn usage_error(&self, id: &str) -> Option<String> {
        (id == "b").then(|| "The saved login has expired.".to_string())
    }
    fn lanes(&self) -> Vec<LaneInfo> {
        Vec::new()
    }
    fn switch_to(&self, _: &str) -> Result<(), String> {
        panic!("a switch was made");
    }
    fn switch_lane(&self, _: &str, _: &str) -> Result<(), String> {
        panic!("a lane was switched");
    }
}

fn op(wait_ms: u64, source: Arc<Fake>) -> ClaudeUsage {
    let op = ClaudeUsage::default().with_wait(Duration::from_millis(wait_ms));
    op.set_source(source);
    op
}

#[test]
fn it_lists_each_accounts_report_and_why_another_has_none() {
    let fake = Arc::new(Fake::new());
    let op = op(100, fake.clone());
    let reply = op.call(true, &json!({})).unwrap();
    assert_eq!(reply["accounts_on"], true);
    assert_eq!(reply["reports"]["a"]["fetched"], 1_800_000_000.0);
    let bars = &reply["reports"]["a"]["groups"][0]["bars"];
    assert_eq!(bars[0]["label"], "5 hours");
    assert_eq!(bars[0]["percent"], 42.0);
    assert_eq!(bars[1]["label"], "Week");
    assert!(reply["reports"].get("b").is_none());
    assert_eq!(reply["errors"]["b"], "The saved login has expired.");
    assert!(reply.get("refreshing").is_none());
    assert_eq!(fake.forced.load(Ordering::SeqCst), 0, "a read reads only");
}

#[test]
fn off_it_reads_nothing_and_lists_nothing() {
    let fake = Arc::new(Fake::new());
    let op = op(100, fake.clone());
    let reply = op.call(false, &json!({"refresh": true})).unwrap();
    assert_eq!(
        reply,
        json!({"accounts_on": false, "reports": {}, "errors": {}})
    );
    assert_eq!(fake.forced.load(Ordering::SeqCst), 0);
    let bare = ClaudeUsage::default();
    assert_eq!(bare.call(true, &json!({})).unwrap()["reports"], json!({}));
}

#[test]
fn refresh_reads_every_account_now_and_waits_for_it() {
    let fake = Arc::new(Fake::new());
    let op = op(2_000, fake.clone());
    let reply = op.call(true, &json!({"refresh": true})).unwrap();
    assert_eq!(fake.forced.load(Ordering::SeqCst), 1);
    assert!(reply.get("refreshing").is_none());
    assert_eq!(
        reply["reports"]["a"]["groups"][0]["bars"][0]["percent"],
        42.0
    );
}

#[test]
fn a_slow_refresh_says_so_and_a_second_one_starts_no_other() {
    let fake = Arc::new(Fake::new());
    let (hold_tx, hold_rx) = std::sync::mpsc::channel();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    *fake.hold.lock().unwrap() = Some(hold_rx);
    *fake.entered.lock().unwrap() = Some(entered_tx);
    let op = op(50, fake.clone());

    let first = op.call(true, &json!({"refresh": true})).unwrap();
    assert_eq!(first["refreshing"], true);
    entered_rx.recv().unwrap();
    let second = op.call(true, &json!({"refresh": true})).unwrap();
    assert_eq!(second["refreshing"], true);
    let asked = op.call(true, &json!({})).unwrap();
    assert_eq!(asked["refreshing"], true);
    assert_eq!(fake.forced.load(Ordering::SeqCst), 1);

    hold_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let reply = op.call(true, &json!({})).unwrap();
        if reply.get("refreshing").is_none() {
            break;
        }
        assert!(Instant::now() < deadline, "the refresh never ended");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fake.forced.load(Ordering::SeqCst), 1);
}
