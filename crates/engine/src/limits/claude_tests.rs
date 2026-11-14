use super::claude::{Announce, ClaudeWatcher, NUDGE};
use super::mode::{LimitConfig, LimitMode, CLAUDE};
use super::sources::{AccountInfo, ClaudeSource, LaneInfo, LimitWhy, StoppedJob, StoppedJobs};
use super::text::Zone;
use super::usage::{AccountUsage, NamedWindow, UsageWindow};
use crate::transcript::errors::LimitHit;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const NOW: f64 = 1_800_000_000.0;
const RESET: f64 = NOW + 3600.0;

fn usage(session: f64, week: f64, fetched: f64) -> AccountUsage {
    AccountUsage {
        session: Some(UsageWindow {
            percent: session,
            resets_at: Some(RESET),
        }),
        week: Some(UsageWindow {
            percent: week,
            resets_at: Some(RESET + 86_400.0),
        }),
        fetched,
        ..AccountUsage::default()
    }
}

#[derive(Default)]
struct Store {
    active: Mutex<Option<String>>,
    usage: Mutex<HashMap<String, AccountUsage>>,
    signed_out: Mutex<Vec<String>>,
    lanes: Mutex<Vec<LaneInfo>>,
    switched: Mutex<Vec<String>>,
    lane_switched: Mutex<Vec<(String, String)>>,
    whys: Mutex<Vec<LimitWhy>>,
    fail: Mutex<bool>,
}

impl Store {
    fn new(active: &str, usage: &[(&str, AccountUsage)]) -> Arc<Store> {
        let s = Store::default();
        *s.active.lock().unwrap() = Some(active.to_string());
        for (id, u) in usage {
            s.usage.lock().unwrap().insert(id.to_string(), u.clone());
        }
        Arc::new(s)
    }
    fn set_usage(&self, id: &str, u: AccountUsage) {
        self.usage.lock().unwrap().insert(id.to_string(), u);
    }
}

impl ClaudeSource for Store {
    fn sync_usage(&self) {}
    fn accounts(&self) -> Vec<AccountInfo> {
        let mut ids: Vec<String> = self.usage.lock().unwrap().keys().cloned().collect();
        ids.sort();
        ids.into_iter()
            .map(|id| AccountInfo {
                email: format!("{id}@x.com"),
                needs_login: self.signed_out.lock().unwrap().contains(&id),
                id,
            })
            .collect()
    }
    fn active_id(&self) -> Option<String> {
        self.active.lock().unwrap().clone()
    }
    fn usage(&self, id: &str) -> Option<AccountUsage> {
        self.usage.lock().unwrap().get(id).cloned()
    }
    fn lanes(&self) -> Vec<LaneInfo> {
        self.lanes.lock().unwrap().clone()
    }
    fn switch_to(&self, id: &str) -> Result<(), String> {
        if *self.fail.lock().unwrap() {
            return Err("the Keychain said no".into());
        }
        self.switched.lock().unwrap().push(id.to_string());
        *self.active.lock().unwrap() = Some(id.to_string());
        Ok(())
    }
    fn switch_for_limit(&self, id: &str, why: &LimitWhy) -> Result<(), String> {
        self.whys.lock().unwrap().push(*why);
        self.switch_to(id)
    }
    fn switch_lane(&self, lane: &str, to: &str) -> Result<(), String> {
        self.lane_switched
            .lock()
            .unwrap()
            .push((lane.to_string(), to.to_string()));
        Ok(())
    }
}

#[derive(Default)]
struct Jobs {
    stopped: Mutex<Vec<StoppedJob>>,
    nudges: Mutex<Vec<(String, String)>>,
    busy: Mutex<bool>,
}

impl StoppedJobs for Jobs {
    fn stopped(&self) -> Vec<StoppedJob> {
        self.stopped.lock().unwrap().clone()
    }
    fn nudge(&self, job_id: &str, text: &str) -> bool {
        if *self.busy.lock().unwrap() {
            return false;
        }
        self.nudges
            .lock()
            .unwrap()
            .push((job_id.to_string(), text.to_string()));
        true
    }
}

struct Rig {
    store: Arc<Store>,
    jobs: Arc<Jobs>,
    said: Arc<Mutex<Vec<String>>>,
    config: Arc<LimitConfig>,
    watcher: ClaudeWatcher,
}

fn rig(store: Arc<Store>, mode: LimitMode) -> Rig {
    let config = Arc::new(LimitConfig::in_memory());
    config.set_mode(&CLAUDE, mode).unwrap();
    let said = Arc::new(Mutex::new(Vec::new()));
    let sink = said.clone();
    let announce: Announce = Arc::new(move |t: &str| sink.lock().unwrap().push(t.to_string()));
    let jobs = Arc::new(Jobs::default());
    let watcher = ClaudeWatcher::new(store.clone(), jobs.clone(), config.clone(), announce)
        .with_zone(Zone::UTC);
    Rig {
        store,
        jobs,
        said,
        config,
        watcher,
    }
}

fn said(r: &Rig) -> Vec<String> {
    r.said.lock().unwrap().clone()
}

fn two_accounts(me: f64) -> Arc<Store> {
    Store::new(
        "me",
        &[
            ("me", usage(me, 10.0, NOW)),
            ("other", usage(10.0, 10.0, NOW)),
        ],
    )
}

#[test]
fn notify_announces_each_level_once_with_the_reset_time() {
    let mut r = rig(two_accounts(91.0), LimitMode::Notify);
    r.watcher.check(NOW);
    r.watcher.check(NOW + 3.0);
    let said_now = said(&r);
    assert_eq!(said_now.len(), 1);
    assert!(
        said_now[0].starts_with("me@x.com is at 91% of its 5-hour limit, resets "),
        "{}",
        said_now[0]
    );
    r.store.set_usage("me", usage(100.0, 10.0, NOW));
    r.watcher.check(NOW + 6.0);
    assert!(said(&r)[1].starts_with("me@x.com has used its 5-hour limit (100%), resets "));
    assert!(r.store.switched.lock().unwrap().is_empty());
}

#[test]
fn off_and_stale_readings_say_nothing() {
    let mut off = rig(two_accounts(99.0), LimitMode::Off);
    off.watcher.check(NOW);
    let mut stale = rig(
        Store::new("me", &[("me", usage(99.0, 99.0, NOW - 15.0 * 60.0 - 1.0))]),
        LimitMode::NotifyAndSwitch,
    );
    stale.watcher.check(NOW);
    assert!(said(&off).is_empty() && said(&stale).is_empty());
    assert!(off.store.switched.lock().unwrap().is_empty());
    assert!(stale.store.switched.lock().unwrap().is_empty());
}

#[test]
fn an_unknown_account_wide_limit_counts() {
    let mut u = usage(10.0, 10.0, NOW);
    u.other.push(NamedWindow {
        name: "Daily Requests".into(),
        window: UsageWindow {
            percent: 100.0,
            resets_at: None,
        },
    });
    let mut r = rig(Store::new("me", &[("me", u)]), LimitMode::Notify);
    r.watcher.check(NOW);
    assert_eq!(
        said(&r),
        ["me@x.com has used its Daily Requests limit (100%)."]
    );
}

#[test]
fn notify_and_switch_moves_to_the_account_with_room_and_says_so_once() {
    let mut r = rig(two_accounts(96.0), LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    assert_eq!(*r.store.switched.lock().unwrap(), ["other"]);
    assert_eq!(
        said(&r),
        ["Switched from me@x.com to other@x.com (10% used) because of the usage limit."],
        "the switch is said, not the climb"
    );
    r.store.set_usage("other", usage(97.0, 10.0, NOW + 60.0));
    r.store.set_usage("me", usage(5.0, 5.0, NOW + 60.0));
    r.watcher.check(NOW + 60.0);
    assert_eq!(r.store.switched.lock().unwrap().len(), 1);
}

#[test]
fn the_switch_says_which_window_crossed_and_how_far() {
    let mut r = rig(two_accounts(96.0), LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    let whys = r.store.whys.lock().unwrap().clone();
    assert_eq!(whys.len(), 1);
    assert_eq!(
        whys[0].window,
        Some(notchling_events::kinds::choices::LimitWindow::FiveHour)
    );
    assert_eq!(whys[0].used_pct, Some(96.0));
}

#[test]
fn skip90_when_the_switch_point_is_at_or_under_90() {
    let mut r = rig(two_accounts(91.0), LimitMode::NotifyAndSwitch);
    r.config.set_switch_at(90).unwrap();
    r.watcher.check(NOW);
    assert_eq!(*r.store.switched.lock().unwrap(), ["other"]);
    assert_eq!(said(&r).len(), 1, "no separate warning: {:?}", said(&r));
}

#[test]
fn no_switch_below_the_switch_point_but_the_climb_is_said() {
    let mut r = rig(two_accounts(92.0), LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    assert!(r.store.switched.lock().unwrap().is_empty());
    assert!(said(&r)[0].contains("at 92%"));
}

#[test]
fn says_when_no_account_has_room_at_most_every_half_hour() {
    let store = Store::new(
        "me",
        &[
            ("me", usage(96.0, 10.0, NOW)),
            ("other", usage(90.0, 10.0, NOW)),
        ],
    );
    let mut r = rig(store, LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    let first = said(&r);
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(
        first[0]
            .starts_with("me@x.com is near its limit and no other account has room left; resets "),
        "{}",
        first[0]
    );
    r.watcher.check(NOW + 60.0);
    assert_eq!(said(&r).len(), 1, "once per half hour");
    r.store
        .set_usage("me", usage(96.0, 10.0, NOW + 31.0 * 60.0));
    r.store
        .set_usage("other", usage(90.0, 10.0, NOW + 31.0 * 60.0));
    r.watcher.check(NOW + 31.0 * 60.0);
    assert_eq!(said(&r).len(), 2);
    assert!(r.store.switched.lock().unwrap().is_empty());
}

#[test]
fn signed_out_stale_and_in_lane_accounts_are_not_targets() {
    let store = Store::new(
        "me",
        &[
            ("me", usage(96.0, 10.0, NOW)),
            ("out", usage(1.0, 1.0, NOW)),
            ("stale", usage(1.0, 1.0, NOW - 11.0 * 60.0)),
            ("lane", usage(1.0, 1.0, NOW)),
        ],
    );
    store.signed_out.lock().unwrap().push("out".into());
    store.lanes.lock().unwrap().push(LaneInfo {
        name: "work".into(),
        path: "/l/work".into(),
        account_id: "lane".into(),
        email: "lane@x.com".into(),
    });
    let mut r = rig(store, LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    assert!(r.store.switched.lock().unwrap().is_empty());
    assert!(said(&r)[0].contains("no other account has room"));
}

#[test]
fn a_failed_switch_is_said_and_retried_without_the_half_hour_wait() {
    let store = two_accounts(96.0);
    *store.fail.lock().unwrap() = true;
    let mut r = rig(store, LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    assert_eq!(said(&r), ["Couldn't switch accounts: the Keychain said no"]);
    *r.store.fail.lock().unwrap() = false;
    r.watcher.check(NOW + 5.0);
    assert_eq!(*r.store.switched.lock().unwrap(), ["other"]);
}

#[test]
fn the_account_left_behind_stays_blocked_until_its_reset() {
    let mut r = rig(two_accounts(96.0), LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    let later = NOW + 31.0 * 60.0 + 3600.0;
    r.store.set_usage("other", usage(97.0, 10.0, later));
    r.store.set_usage("me", usage(2.0, 2.0, later));
    r.watcher.check(later);
    assert_eq!(*r.store.switched.lock().unwrap(), ["other", "me"]);

    let mut r = rig(two_accounts(96.0), LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    let soon = NOW + 31.0 * 60.0;
    r.store.set_usage("other", usage(97.0, 10.0, soon));
    r.store.set_usage("me", usage(2.0, 2.0, soon));
    r.watcher.check(soon);
    assert_eq!(
        *r.store.switched.lock().unwrap(),
        ["other"],
        "me hasn't reset yet, so it isn't taken back"
    );
}

#[test]
fn a_lane_switches_on_its_own_to_an_account_live_nowhere() {
    let store = Store::new(
        "main",
        &[
            ("main", usage(5.0, 5.0, NOW)),
            ("laned", usage(96.0, 5.0, NOW)),
            ("free", usage(20.0, 5.0, NOW)),
        ],
    );
    store.lanes.lock().unwrap().push(LaneInfo {
        name: "work".into(),
        path: "/l/work".into(),
        account_id: "laned".into(),
        email: "laned@x.com".into(),
    });
    let mut r = rig(store, LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    assert_eq!(
        *r.store.lane_switched.lock().unwrap(),
        [("work".to_string(), "free".to_string())]
    );
    assert!(
        r.store.switched.lock().unwrap().is_empty(),
        "the main login stays"
    );
    assert_eq!(
        said(&r),
        ["The work lane switched from laned@x.com to free@x.com because of the usage limit. Its sessions carry on."]
    );
}

#[test]
fn a_lane_never_takes_the_account_the_main_login_holds() {
    let store = Store::new(
        "main",
        &[
            ("main", usage(1.0, 1.0, NOW)),
            ("laned", usage(96.0, 5.0, NOW)),
        ],
    );
    store.lanes.lock().unwrap().push(LaneInfo {
        name: "work".into(),
        path: "/l/work".into(),
        account_id: "laned".into(),
        email: "laned@x.com".into(),
    });
    let mut r = rig(store, LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    assert!(r.store.lane_switched.lock().unwrap().is_empty());
    assert_eq!(
        said(&r),
        ["The work lane (laned@x.com) is near its limit and no free account has room left."]
    );
}

#[test]
fn lanes_do_not_switch_in_notify_mode() {
    let store = Store::new(
        "main",
        &[
            ("main", usage(1.0, 1.0, NOW)),
            ("laned", usage(96.0, 5.0, NOW)),
            ("free", usage(1.0, 1.0, NOW)),
        ],
    );
    store.lanes.lock().unwrap().push(LaneInfo {
        name: "work".into(),
        path: "/l/work".into(),
        account_id: "laned".into(),
        email: "laned@x.com".into(),
    });
    let mut r = rig(store, LimitMode::Notify);
    r.watcher.check(NOW);
    assert!(r.store.lane_switched.lock().unwrap().is_empty());
}

fn stopped(job: &str, config_dir: Option<&str>, id: &str, at: f64) -> StoppedJob {
    StoppedJob {
        job_id: job.into(),
        config_dir: config_dir.map(String::from),
        hit: LimitHit {
            id: id.into(),
            at: Some(at),
            text: "You've hit your session limit".into(),
        },
    }
}

#[test]
fn a_job_stopped_on_the_limit_triggers_a_switch_and_is_nudged_once_after_it() {
    let mut r = rig(two_accounts(50.0), LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW - 100.0);
    r.jobs
        .stopped
        .lock()
        .unwrap()
        .push(stopped("j1", None, "e1", NOW - 30.0));
    r.watcher.check(NOW + 20.0);
    assert_eq!(*r.store.switched.lock().unwrap(), ["other"]);
    assert!(
        r.jobs.nudges.lock().unwrap().is_empty(),
        "the next round nudges"
    );
    r.watcher.check(NOW + 40.0);
    assert_eq!(
        *r.jobs.nudges.lock().unwrap(),
        [("j1".to_string(), NUDGE.to_string())]
    );
    r.watcher.check(NOW + 60.0);
    assert_eq!(
        r.jobs.nudges.lock().unwrap().len(),
        1,
        "one nudge per error entry"
    );
}

#[test]
fn an_error_older_than_the_account_in_use_is_someone_elses() {
    let mut r = rig(two_accounts(50.0), LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    r.jobs
        .stopped
        .lock()
        .unwrap()
        .push(stopped("j1", None, "e1", NOW - 500.0));
    r.watcher.check(NOW + 60.0);
    assert!(r.store.switched.lock().unwrap().is_empty());
}

#[test]
fn only_jobs_the_switch_moved_are_nudged() {
    let store = two_accounts(96.0);
    store.lanes.lock().unwrap().push(LaneInfo {
        name: "work".into(),
        path: "/l/work".into(),
        account_id: "elsewhere".into(),
        email: "e@x.com".into(),
    });
    let mut r = rig(store, LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    assert_eq!(r.store.switched.lock().unwrap().len(), 1);
    r.jobs.stopped.lock().unwrap().extend([
        stopped("main-job", None, "e1", NOW - 60.0),
        stopped("lane-job", Some("/l/work"), "e2", NOW - 60.0),
    ]);
    r.watcher.check(NOW + 30.0);
    let nudged: Vec<_> = r
        .jobs
        .nudges
        .lock()
        .unwrap()
        .iter()
        .map(|n| n.0.clone())
        .collect();
    assert_eq!(nudged, ["main-job"], "the lane did not switch");
}

#[test]
fn a_job_already_being_restarted_is_tried_again_and_old_errors_are_left() {
    let mut r = rig(two_accounts(96.0), LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    *r.jobs.busy.lock().unwrap() = true;
    r.jobs.stopped.lock().unwrap().extend([
        stopped("j1", None, "e1", NOW - 60.0),
        stopped("ancient", None, "e0", NOW - 13.0 * 3600.0),
    ]);
    r.watcher.check(NOW + 30.0);
    assert!(r.jobs.nudges.lock().unwrap().is_empty());
    *r.jobs.busy.lock().unwrap() = false;
    r.watcher.check(NOW + 50.0);
    let nudged: Vec<_> = r
        .jobs
        .nudges
        .lock()
        .unwrap()
        .iter()
        .map(|n| n.0.clone())
        .collect();
    assert_eq!(nudged, ["j1"]);
}

#[test]
fn nobody_is_nudged_two_hours_after_the_switch() {
    let mut r = rig(two_accounts(96.0), LimitMode::NotifyAndSwitch);
    r.watcher.check(NOW);
    r.jobs
        .stopped
        .lock()
        .unwrap()
        .push(stopped("j1", None, "e1", NOW + 2.0 * 3600.0 - 10.0));
    r.store
        .set_usage("other", usage(5.0, 5.0, NOW + 2.0 * 3600.0 + 1.0));
    r.watcher.check(NOW + 2.0 * 3600.0 + 1.0);
    assert!(r.jobs.nudges.lock().unwrap().is_empty());
}
