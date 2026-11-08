//! Port of the watcher half of `ProviderLimitsTests`, against a fake provider that only records
//! the switch it is asked for.

use super::claude::Announce;
use super::mode::{LimitConfig, LimitMode, ANTIGRAVITY, CODEX};
use super::provider::{ProviderLimitWatcher, Setup};
use super::report::{Bar, BarKind, Group, UsageReport};
use super::sources::{ProviderAccountInfo, ProviderSource};
use super::text::Zone;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const NOW: f64 = 1_800_000_000.0;
const RESET: f64 = NOW + 3600.0;

fn report(session: f64, week: f64, fetched: f64) -> UsageReport {
    let bar = |label: &str, kind, percent, resets_at| Bar {
        label: label.into(),
        kind,
        percent,
        resets_at: Some(resets_at),
    };
    UsageReport {
        groups: vec![Group {
            name: Some("Codex".into()),
            bars: vec![
                bar("5h", BarKind::Session, session, RESET),
                bar("Week", BarKind::Week, week, RESET + 86_400.0),
            ],
        }],
        fetched,
    }
}

#[derive(Default)]
struct Fake {
    reports: Mutex<HashMap<String, UsageReport>>,
    switched: Mutex<Vec<String>>,
    fail: Mutex<bool>,
    synced: Mutex<u32>,
}

impl Fake {
    fn new(reports: &[(&str, UsageReport)]) -> Arc<Fake> {
        let fake = Fake::default();
        for (id, r) in reports {
            fake.reports
                .lock()
                .unwrap()
                .insert(id.to_string(), r.clone());
        }
        Arc::new(fake)
    }
}

impl ProviderSource for Fake {
    fn sync_usage(&self) {
        *self.synced.lock().unwrap() += 1;
    }
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
        self.reports.lock().unwrap().get(id).cloned()
    }
    fn switch_to(&self, id: &str) -> Result<(), String> {
        if *self.fail.lock().unwrap() {
            return Err("the provider said no".into());
        }
        self.switched.lock().unwrap().push(id.to_string());
        Ok(())
    }
}

struct Rig {
    fake: Arc<Fake>,
    said: Arc<Mutex<Vec<String>>>,
    config: Arc<LimitConfig>,
    watcher: ProviderLimitWatcher,
}

fn rig(fake: Arc<Fake>, setup: Setup, mode: LimitMode, idle: bool) -> Rig {
    let config = Arc::new(LimitConfig::in_memory());
    if config.set_mode(&setup.provider, mode).is_err() {
        // A mode the provider doesn't allow stays unset: the default reads.
    }
    let said = Arc::new(Mutex::new(Vec::new()));
    let sink = said.clone();
    let announce: Announce = Arc::new(move |t: &str| sink.lock().unwrap().push(t.to_string()));
    let watcher = ProviderLimitWatcher::new(
        fake.clone(),
        setup,
        config.clone(),
        Box::new(|| 95),
        Box::new(move || idle),
        announce,
    )
    .with_zone(Zone::UTC);
    Rig {
        fake,
        said,
        config,
        watcher,
    }
}

fn codex(fake: Arc<Fake>, mode: LimitMode, idle: bool) -> Rig {
    rig(fake, Setup::codex(), mode, idle)
}

fn said(rig: &Rig) -> Vec<String> {
    rig.said.lock().unwrap().clone()
}

#[test]
fn notify_announces_each_level_once_and_never_switches() {
    let fake = Fake::new(&[
        ("me", report(96.0, 10.0, NOW)),
        ("other", report(1.0, 1.0, NOW)),
    ]);
    let mut r = codex(fake, LimitMode::Notify, true);
    r.watcher.check(NOW);
    r.watcher.check(NOW + 3.0);
    let said_now = said(&r);
    assert_eq!(said_now.len(), 1);
    assert!(
        said_now[0].contains("at 96% of its 5-hour limit"),
        "{}",
        said_now[0]
    );
    assert!(
        said_now[0].starts_with("Codex: me@x.com"),
        "{}",
        said_now[0]
    );
    r.fake
        .reports
        .lock()
        .unwrap()
        .insert("me".into(), report(100.0, 10.0, NOW));
    r.watcher.check(NOW + 6.0);
    let said_now = said(&r);
    assert_eq!(said_now.len(), 2);
    assert!(
        said_now[1].contains("has used its 5-hour limit"),
        "{}",
        said_now[1]
    );
    assert!(r.fake.switched.lock().unwrap().is_empty());
}

#[test]
fn off_and_stale_readings_say_nothing() {
    let fake = Fake::new(&[("me", report(99.0, 99.0, NOW))]);
    let mut off = codex(fake.clone(), LimitMode::Off, true);
    off.watcher.check(NOW);
    let stale = report(99.0, 99.0, NOW - 15.0 * 60.0 - 1.0);
    fake.reports.lock().unwrap().insert("me".into(), stale);
    let mut notify = codex(fake, LimitMode::Notify, true);
    notify.watcher.check(NOW);
    assert!(said(&off).is_empty() && said(&notify).is_empty());
}

#[test]
fn the_default_modes_are_the_swift_apps() {
    let config = LimitConfig::in_memory();
    assert_eq!(
        config.mode(&super::mode::CLAUDE),
        LimitMode::NotifyAndSwitch
    );
    assert_eq!(config.mode(&CODEX), LimitMode::Notify);
    assert_eq!(config.mode(&ANTIGRAVITY), LimitMode::Notify);
    assert_eq!(config.switch_at(), 95);
}

#[test]
fn a_mode_the_provider_does_not_allow_is_refused_and_the_default_reads() {
    let r = rig(
        Fake::new(&[]),
        Setup::antigravity(),
        LimitMode::NotifyAndSwitch,
        true,
    );
    assert!(r
        .config
        .set_mode(&ANTIGRAVITY, LimitMode::NotifyAndSwitch)
        .is_err());
    assert_eq!(r.watcher.mode(), LimitMode::Notify);
    r.config.set_mode(&ANTIGRAVITY, LimitMode::Off).unwrap();
    assert_eq!(r.watcher.mode(), LimitMode::Off);
}

#[test]
fn notify_and_switch_asks_the_provider_to_switch_when_idle() {
    let fake = Fake::new(&[
        ("me", report(96.0, 10.0, NOW)),
        ("other", report(10.0, 10.0, NOW)),
    ]);
    let mut r = codex(fake, LimitMode::NotifyAndSwitch, true);
    r.watcher.check(NOW);
    assert_eq!(*r.fake.switched.lock().unwrap(), ["other"]);
    let said_now = said(&r);
    assert_eq!(
        said_now.len(),
        1,
        "only the switch is said, not the climb: {said_now:?}"
    );
    assert!(
        said_now[0].starts_with("Codex: switched from me@x.com to other@x.com (10% used)"),
        "{}",
        said_now[0]
    );
    // Half an hour must pass before another.
    r.fake
        .reports
        .lock()
        .unwrap()
        .insert("me".into(), report(97.0, 10.0, NOW + 60.0));
    r.watcher.check(NOW + 60.0);
    assert_eq!(r.fake.switched.lock().unwrap().len(), 1);
}

#[test]
fn notify_and_switch_only_notifies_while_busy_and_says_so() {
    let fake = Fake::new(&[
        ("me", report(96.0, 10.0, NOW)),
        ("other", report(10.0, 10.0, NOW)),
    ]);
    let mut r = codex(fake, LimitMode::NotifyAndSwitch, false);
    r.watcher.check(NOW);
    r.watcher.check(NOW + 3.0);
    assert!(r.fake.switched.lock().unwrap().is_empty());
    let said_now = said(&r);
    assert_eq!(said_now.len(), 1);
    assert!(
        said_now[0].contains("A Codex session is running"),
        "{}",
        said_now[0]
    );
}

#[test]
fn notify_and_switch_says_when_no_other_account_has_room() {
    let fake = Fake::new(&[
        ("me", report(96.0, 10.0, NOW)),
        ("other", report(90.0, 10.0, NOW)),
    ]);
    let mut r = codex(fake, LimitMode::NotifyAndSwitch, true);
    r.watcher.check(NOW);
    assert!(r.fake.switched.lock().unwrap().is_empty());
    let said_now = said(&r);
    assert_eq!(said_now.len(), 1);
    assert!(
        said_now[0].contains("No other Codex account has room"),
        "{}",
        said_now[0]
    );
}

#[test]
fn a_failed_switch_is_said_and_not_remembered() {
    let fake = Fake::new(&[
        ("me", report(96.0, 10.0, NOW)),
        ("other", report(10.0, 10.0, NOW)),
    ]);
    *fake.fail.lock().unwrap() = true;
    let mut r = codex(fake, LimitMode::NotifyAndSwitch, true);
    r.watcher.check(NOW);
    assert!(said(&r)[0].contains("couldn't switch accounts: the provider said no"));
    *r.fake.fail.lock().unwrap() = false;
    r.watcher.check(NOW + 3.0);
    assert_eq!(
        *r.fake.switched.lock().unwrap(),
        ["other"],
        "tried again, no half hour wait"
    );
}

#[test]
fn the_view_for_limits_status_keeps_the_switch() {
    let fake = Fake::new(&[
        ("me", report(96.0, 10.0, NOW)),
        ("other", report(10.0, 10.0, NOW)),
    ]);
    let mut r = codex(fake, LimitMode::NotifyAndSwitch, true);
    r.watcher.check(NOW);
    let seen = r.watcher.observed();
    assert_eq!(seen.last_switch, Some(NOW));
    assert_eq!(seen.active_email.as_deref(), Some("me@x.com"));
    assert_eq!(seen.windows.len(), 2);
}
