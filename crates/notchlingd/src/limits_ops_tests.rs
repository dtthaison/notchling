//! The limit ops and the watcher loop over fakes. The fakes that stand for a login, a usage
//! request or a switch panic when called, so a test that leaves `accounts` off proves nothing ran.

use crate::acting_ops_tests::FakeRoom;
use crate::cost_ops::CostSetup;
use crate::limits_ops::*;
use crate::switches::{SwitchFlag, ACCOUNTS, ACCOUNTS_OFF};
use notchling_engine::cost::tracker::{Root, CLAUDE_CODE};
use notchling_engine::limits::mode::{LimitConfig, LimitMode, ANTIGRAVITY, CLAUDE};
use notchling_engine::limits::report::{Bar, BarKind, Group, UsageReport};
use notchling_engine::limits::sources::{
    AccountInfo, ClaudeSource, LaneInfo, ProviderAccountInfo, ProviderSource, StoppedJob,
    StoppedJobs,
};
use notchling_engine::limits::usage::{AccountUsage, UsageWindow};
use notchling_local_api::{ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(crate) const NOW: f64 = 1_800_000_000.0;

/// A scratch folder under the system temp folder, removed on drop.
pub(crate) struct TempDir(std::path::PathBuf);

impl TempDir {
    pub(crate) fn new(tag: &str) -> TempDir {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("nl-limits-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    pub(crate) fn root(&self) -> &std::path::Path {
        &self.0
    }

    pub(crate) fn path(&self, rel: &str) -> std::path::PathBuf {
        self.0.join(rel)
    }

    pub(crate) fn write(&self, rel: &str, text: &str) {
        let path = self.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Panics on any call: the stand-in for a store that must not be touched.
struct Untouchable;

impl ClaudeSource for Untouchable {
    fn sync_usage(&self) {
        panic!("a usage request was made");
    }
    fn accounts(&self) -> Vec<AccountInfo> {
        panic!("accounts were read");
    }
    fn active_id(&self) -> Option<String> {
        panic!("the login was read");
    }
    fn usage(&self, _: &str) -> Option<AccountUsage> {
        panic!("usage was read");
    }
    fn lanes(&self) -> Vec<LaneInfo> {
        panic!("lanes were read");
    }
    fn switch_to(&self, _: &str) -> Result<(), String> {
        panic!("a switch was made");
    }
    fn switch_lane(&self, _: &str, _: &str) -> Result<(), String> {
        panic!("a lane was switched");
    }
}

impl StoppedJobs for Untouchable {
    fn stopped(&self) -> Vec<StoppedJob> {
        panic!("transcripts were scanned");
    }
    fn nudge(&self, _: &str, _: &str) -> bool {
        panic!("a job was nudged");
    }
}

impl ProviderSource for Untouchable {
    fn sync_usage(&self) {
        panic!("a usage request was made");
    }
    fn accounts(&self) -> Vec<ProviderAccountInfo> {
        panic!("accounts were read");
    }
    fn active_id(&self) -> Option<String> {
        panic!("the login was read");
    }
    fn usage_report(&self, _: &str) -> Option<UsageReport> {
        panic!("usage was read");
    }
    fn switch_to(&self, _: &str) -> Result<(), String> {
        panic!("a switch was made");
    }
}

/// A Claude store with two accounts; the active one is at `me` percent.
#[derive(Default)]
struct Claude {
    me: Mutex<f64>,
    syncs: AtomicU32,
    switched: Mutex<Vec<String>>,
}

fn usage(session: f64) -> AccountUsage {
    AccountUsage {
        session: Some(UsageWindow {
            percent: session,
            resets_at: Some(NOW + 3600.0),
        }),
        week: Some(UsageWindow {
            percent: 5.0,
            resets_at: None,
        }),
        fetched: NOW,
        ..AccountUsage::default()
    }
}

impl ClaudeSource for Claude {
    fn sync_usage(&self) {
        self.syncs.fetch_add(1, Ordering::SeqCst);
    }
    fn accounts(&self) -> Vec<AccountInfo> {
        ["me", "other"]
            .iter()
            .map(|id| AccountInfo {
                id: id.to_string(),
                email: format!("{id}@x.com"),
                needs_login: false,
            })
            .collect()
    }
    fn active_id(&self) -> Option<String> {
        Some("me".into())
    }
    fn usage(&self, id: &str) -> Option<AccountUsage> {
        Some(usage(if id == "me" {
            *self.me.lock().unwrap()
        } else {
            10.0
        }))
    }
    fn lanes(&self) -> Vec<LaneInfo> {
        Vec::new()
    }
    fn switch_to(&self, id: &str) -> Result<(), String> {
        self.switched.lock().unwrap().push(id.to_string());
        Ok(())
    }
    fn switch_lane(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

struct NoJobs;

impl StoppedJobs for NoJobs {
    fn stopped(&self) -> Vec<StoppedJob> {
        Vec::new()
    }
    fn nudge(&self, _: &str, _: &str) -> bool {
        false
    }
}

fn ops_with(tmp: &TempDir, flag: Arc<SwitchFlag>, room: Arc<FakeRoom>) -> LimitsOps {
    let cost = CostSetup {
        roots: vec![Root {
            dir: tmp.path("claude/projects"),
            channel: CLAUDE_CODE,
        }],
        cache_data: None,
        account_log: tmp.path("account-log.json"),
    }
    .service();
    LimitsOps::new(flag, Arc::new(LimitConfig::in_memory()), cost, room)
        .with_clock(Arc::new(|| NOW))
}

pub(crate) fn rig() -> (TempDir, Arc<SwitchFlag>, Arc<FakeRoom>, LimitsOps) {
    let tmp = TempDir::new("limits-ops");
    let flag = SwitchFlag::off(ACCOUNTS, ACCOUNTS_OFF);
    let room = Arc::new(FakeRoom::default());
    let ops = ops_with(&tmp, flag.clone(), room.clone());
    (tmp, flag, room, ops)
}

#[test]
fn the_switch_is_off_at_every_start() {
    assert!(!SwitchFlag::off(ACCOUNTS, ACCOUNTS_OFF).is_on());
}

#[test]
fn with_accounts_off_no_store_is_ever_touched() {
    let (_tmp, _flag, room, ops) = rig();
    ops.set_claude(Arc::new(Untouchable), Arc::new(Untouchable));
    ops.set_codex(
        Arc::new(Untouchable),
        Box::new(|| panic!("the process list was read")),
    );
    ops.set_antigravity(Arc::new(Untouchable));
    for i in 0..5 {
        assert!(!ops.tick_at(NOW + f64::from(i) * 400.0), "nothing ran");
    }
    // Asking for the status or setting a mode reads no store either.
    let status = ops.call(LIMITS_STATUS, &json!({})).unwrap();
    assert_eq!(status["accounts_on"], json!(false));
    assert_eq!(status["watching"], json!(false));
    ops.call(
        LIMITS_SET_MODE,
        &json!({"provider": "claude", "mode": "notify"}),
    )
    .unwrap();
    assert!(room.events.lock().unwrap().is_empty());
}

#[test]
fn the_loop_thread_does_nothing_while_off_and_works_once_on() {
    let tmp = TempDir::new("limits-loop");
    let flag = SwitchFlag::off(ACCOUNTS, ACCOUNTS_OFF);
    let ops = Arc::new(
        ops_with(&tmp, flag.clone(), Arc::new(FakeRoom::default()))
            .with_tick(Duration::from_millis(10)),
    );
    let claude = Arc::new(Claude::default());
    *claude.me.lock().unwrap() = 50.0;
    ops.set_claude(Arc::new(Untouchable), Arc::new(Untouchable));
    ops.spawn_loop();
    std::thread::sleep(Duration::from_millis(80));
    // Had the loop touched the untouchable store, its thread would have died there and the wait
    // below would time out.
    ops.set_claude(claude.clone(), Arc::new(NoJobs));
    flag.set(true);
    let deadline = Instant::now() + Duration::from_secs(5);
    while claude.syncs.load(Ordering::SeqCst) == 0 {
        assert!(
            Instant::now() < deadline,
            "the loop did not run once accounts was on"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    flag.set(false);
    ops.close();
    assert_eq!(
        claude.syncs.load(Ordering::SeqCst),
        1,
        "synced once, not every tick"
    );
}

#[test]
fn a_climb_is_said_once_and_sent_as_an_event() {
    let (_tmp, flag, room, ops) = rig();
    let claude = Arc::new(Claude::default());
    *claude.me.lock().unwrap() = 92.0;
    ops.config().set_mode(&CLAUDE, LimitMode::Notify).unwrap();
    ops.set_claude(claude.clone(), Arc::new(NoJobs));
    flag.set(true);
    assert!(ops.tick_at(NOW));
    assert!(ops.tick_at(NOW + 10.0));
    let notices = room.named("limits.notice");
    assert_eq!(notices.len(), 1);
    assert!(notices[0]["text"]
        .as_str()
        .unwrap()
        .contains("at 92% of its 5-hour limit"));
    assert!(
        claude.switched.lock().unwrap().is_empty(),
        "Notify never switches"
    );
    assert_eq!(claude.syncs.load(Ordering::SeqCst), 1);
    // Five minutes on, the stores are asked again.
    ops.tick_at(NOW + 301.0);
    assert_eq!(claude.syncs.load(Ordering::SeqCst), 2);

    let status = ops.call(LIMITS_STATUS, &json!({})).unwrap();
    assert_eq!(status["watching"], json!(true));
    let c = &status["providers"][0];
    assert_eq!(c["id"], json!("claude"));
    assert_eq!(c["registered"], json!(true));
    assert_eq!(c["active"], json!("me@x.com"));
    assert_eq!(c["windows"][0]["name"], json!("5-hour"));
    assert_eq!(status["notices"].as_array().unwrap().len(), 1);
    assert_eq!(status["providers"][1]["registered"], json!(false));
}

#[test]
fn turning_accounts_off_drops_what_was_seen_and_the_next_round_on_syncs_afresh() {
    let (_tmp, flag, _room, ops) = rig();
    let claude = Arc::new(Claude::default());
    *claude.me.lock().unwrap() = 50.0;
    ops.set_claude(claude.clone(), Arc::new(NoJobs));
    flag.set(true);
    assert!(ops.tick_at(NOW));
    assert_eq!(claude.syncs.load(Ordering::SeqCst), 1);
    let status = ops.call(LIMITS_STATUS, &json!({})).unwrap();
    assert_eq!(status["providers"][0]["active"], json!("me@x.com"));

    flag.set(false);
    let status = ops.call(LIMITS_STATUS, &json!({})).unwrap();
    assert_eq!(status["providers"][0]["active"], Value::Null, "forgotten");
    assert_eq!(status["providers"][0]["windows"], json!([]));
    flag.set(true);
    // Within the five minutes, yet the stores are asked again: what they said may be stale.
    assert!(ops.tick_at(NOW + 10.0));
    assert_eq!(claude.syncs.load(Ordering::SeqCst), 2);
}

#[test]
fn notify_and_switch_switches_only_through_the_store_and_only_when_on() {
    let (_tmp, flag, _room, ops) = rig();
    let claude = Arc::new(Claude::default());
    *claude.me.lock().unwrap() = 97.0;
    ops.set_claude(claude.clone(), Arc::new(NoJobs));
    assert!(!ops.tick_at(NOW));
    assert!(claude.switched.lock().unwrap().is_empty());
    flag.set(true);
    ops.tick_at(NOW);
    assert_eq!(*claude.switched.lock().unwrap(), ["other"]);
    flag.set(false);
    *claude.me.lock().unwrap() = 99.0;
    ops.tick_at(NOW + 4000.0);
    assert_eq!(
        claude.switched.lock().unwrap().len(),
        1,
        "off again: no more"
    );
}

#[test]
fn set_mode_checks_everything_before_it_writes() {
    let (_tmp, _flag, _room, ops) = rig();
    let set = |args: Value| ops.call(LIMITS_SET_MODE, &args);
    assert_eq!(
        set(json!({"provider": "codex", "mode": "notifyAndSwitch", "switch_at": 90})).unwrap(),
        json!({"provider": "codex", "mode": "notifyAndSwitch", "switch_at": 90})
    );
    for (args, why) in [
        (
            json!({"provider": "antigravity", "mode": "notifyAndSwitch"}),
            "never switches",
        ),
        (
            json!({"provider": "claude", "mode": "sometimes"}),
            "unknown mode",
        ),
        (
            json!({"provider": "gemini", "mode": "off"}),
            "unknown provider",
        ),
        (json!({"mode": "off"}), "no provider"),
        (json!({"provider": "claude"}), "nothing to set"),
        (
            json!({"provider": "claude", "switch_at": 79}),
            "below the range",
        ),
        (
            json!({"provider": "claude", "switch_at": 100}),
            "above the range",
        ),
        (
            json!({"provider": "claude", "switch_at": "95"}),
            "not a number",
        ),
        (json!({"provider": "claude", "mode": 1}), "not a string"),
        (
            json!({"provider": "antigravity", "mode": "notifyAndSwitch", "switch_at": 85}),
            "all or nothing",
        ),
    ] {
        let e = set(args).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{why}");
    }
    let status = ops.call(LIMITS_STATUS, &json!({})).unwrap();
    assert_eq!(
        status["switch_at"],
        json!(90),
        "a refused call changed nothing"
    );
    assert_eq!(status["providers"][2]["mode"], json!("notify"));
    assert_eq!(
        status["providers"][2]["modes"],
        json!(["off", "notify"]),
        "Antigravity offers Off and Notify"
    );
    assert_eq!(ops.config().mode(&ANTIGRAVITY), LimitMode::Notify);
}

#[test]
fn the_default_modes_are_the_swift_apps() {
    let (_tmp, _flag, _room, ops) = rig();
    let status = ops.call(LIMITS_STATUS, &json!({})).unwrap();
    let modes: Vec<_> = status["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["id"].as_str().unwrap(), p["mode"].as_str().unwrap()))
        .collect();
    assert_eq!(
        modes,
        [
            ("claude", "notifyAndSwitch"),
            ("codex", "notify"),
            ("antigravity", "notify")
        ]
    );
    assert_eq!(status["switch_at"], json!(95));
}

#[test]
fn tiers() {
    let (_tmp, _flag, _room, ops) = rig();
    assert_eq!(ops.spec(LIMITS_STATUS), Some(OpSpec::APP));
    assert_eq!(ops.spec(LIMITS_SET_MODE), Some(OpSpec::APP));
    assert_eq!(ops.spec(COST_SUMMARY), Some(OpSpec::AGENT));
    assert_eq!(ops.spec("limits.switch"), None);
}

#[test]
fn the_codex_watcher_is_notify_by_default() {
    let (_tmp, flag, room, ops) = rig();
    struct Codex(Mutex<f64>, AtomicU32);
    impl ProviderSource for Codex {
        fn sync_usage(&self) {}
        fn accounts(&self) -> Vec<ProviderAccountInfo> {
            ["me", "other"]
                .iter()
                .map(|id| ProviderAccountInfo {
                    id: id.to_string(),
                    email: format!("{id}@x.com"),
                    needs_login: false,
                    switchable: true,
                })
                .collect()
        }
        fn active_id(&self) -> Option<String> {
            Some("me".into())
        }
        fn usage_report(&self, id: &str) -> Option<UsageReport> {
            let percent = if id == "me" {
                *self.0.lock().unwrap()
            } else {
                5.0
            };
            Some(UsageReport {
                groups: vec![Group {
                    name: Some("Codex".into()),
                    bars: vec![Bar {
                        label: "5h".into(),
                        kind: BarKind::Session,
                        percent,
                        resets_at: None,
                    }],
                }],
                fetched: NOW,
            })
        }
        fn switch_to(&self, _: &str) -> Result<(), String> {
            self.1.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    let codex = Arc::new(Codex(Mutex::new(99.0), AtomicU32::new(0)));
    ops.set_codex(codex.clone(), Box::new(|| true));
    flag.set(true);
    ops.tick_at(NOW);
    assert_eq!(
        codex.1.load(Ordering::SeqCst),
        0,
        "Notify by default: no switch"
    );
    let texts: Vec<_> = room.named("limits.notice");
    assert!(texts[0]["text"]
        .as_str()
        .unwrap()
        .starts_with("Codex: me@x.com"));
    // Turned to Notify and Switch, it switches (the process check says idle).
    ops.call(
        LIMITS_SET_MODE,
        &json!({"provider": "codex", "mode": "notifyAndSwitch"}),
    )
    .unwrap();
    ops.tick_at(NOW + 1.0);
    assert_eq!(codex.1.load(Ordering::SeqCst), 1);
}
