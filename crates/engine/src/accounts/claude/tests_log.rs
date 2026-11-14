use super::tests_support::*;
use super::*;
use crate::cost::account_log::{self, Entry};
use notchling_events::MemorySink;

const NOW_SECS: f64 = NOW as f64 / 1000.0;

fn log(w: &World) -> Vec<Entry> {
    SinkHistory(w.sink.clone()).switches()
}

fn ids(w: &World) -> Vec<String> {
    log(w).into_iter().map(|e| e.id).collect()
}

fn file(w: &World) -> std::path::PathBuf {
    w.tmp.path("data/account-log.json")
}

fn causes(sink: &MemorySink) -> Vec<String> {
    switched(sink)
        .iter()
        .filter_map(|e| text(e, "cause"))
        .collect()
}

#[test]
fn a_switch_pushes_an_event_from_the_account_it_left_to_the_one_it_went_to() {
    let w = World::new("log-switch");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.saved(2, 1, LATER);
    w.store.switch_to(&id(2)).unwrap();
    assert_eq!(ids(&w), [id(1), id(2)]);
    assert!(log(&w).iter().all(|e| e.at == NOW_SECS));
    let events = switched(&w.sink);
    assert_eq!(text(&events[0], "from"), None);
    assert_eq!(text(&events[1], "from"), Some(id(1)));
    assert_eq!(text(&events[1], "to"), Some(id(2)));
    assert_eq!(text(&events[1], "provider").as_deref(), Some("claude"));
    assert_eq!(text(&events[1], "result").as_deref(), Some("done"));
    assert_eq!(causes(&w.sink), ["other", "user"]);
    assert!(!file(&w).exists(), "the file is no longer written");
}

#[test]
fn cost_follows_the_events_after_a_switch() {
    let w = World::new("log-cost");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.saved(2, 1, LATER);
    w.store.sync().unwrap();
    w.clock
        .store(NOW + 60_000, std::sync::atomic::Ordering::SeqCst);
    w.store.switch_to(&id(2)).unwrap();
    let log = log(&w);
    assert_eq!(account_log::account_at(NOW_SECS + 30.0, &log), id(1));
    assert_eq!(account_log::account_at(NOW_SECS + 61.0, &log), id(2));
}

#[test]
fn a_sync_notices_a_login_changed_outside_notchling() {
    let w = World::new("log-outside");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.store.sync().unwrap();
    w.clock
        .store(NOW + 60_000, std::sync::atomic::Ordering::SeqCst);
    w.signed_in(2, 1);
    w.answers_profile(2);
    w.store.sync().unwrap();
    assert_eq!(ids(&w), [id(1), id(2)]);
    assert_eq!(log(&w)[1].at, NOW_SECS + 60.0);
    assert_eq!(causes(&w.sink), ["other", "other"]);
}

#[test]
fn the_same_account_is_pushed_once_even_across_restarts() {
    let w = World::new("log-once");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.store.sync().unwrap();
    w.store.sync().unwrap();
    let again = ClaudeAccounts::new(ClaudeEnv {
        keys: w.store.env().keys.clone(),
        undo_keys: w.store.env().undo_keys.clone(),
        http: w.store.env().http.clone(),
        now_ms: Arc::new(|| LATER),
        config_file: w.store.env().config_file.clone(),
        data_dir: w.store.env().data_dir.clone(),
        list_file: w.store.env().list_file.clone(),
        lanes: LaneStore::new(&w.tmp.path("home"), &w.tmp.path("data")),
        events: w.sink.clone(),
        history: Arc::new(SinkHistory(w.sink.clone())),
    });
    again.sync().unwrap();
    assert_eq!(ids(&w), [id(1)]);
}

#[test]
fn a_log_the_swift_app_wrote_is_left_alone() {
    let w = World::new("log-swift");
    let old = r#"[{"at":814681048,"id":"a3a3:0303"},{"at":815978594.1160798,"id":"a4a4:0404"}]"#;
    w.tmp.write("data/account-log.json", old);
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.store.sync().unwrap();
    assert_eq!(std::fs::read_to_string(file(&w)).unwrap(), old);
    assert_eq!(ids(&w), [id(1)]);
}

#[test]
fn an_unreadable_file_changes_nothing_and_the_switch_still_counts() {
    let broken = World::new("log-broken");
    broken.tmp.write("data/account-log.json", "{not a list");
    broken.signed_in(1, 1);
    broken.answers_profile(1);
    assert_eq!(broken.store.sync().unwrap().active, Some(id(1)));
    assert_eq!(
        std::fs::read_to_string(file(&broken)).unwrap(),
        "{not a list"
    );
    assert_eq!(ids(&broken), [id(1)]);
}

#[test]
fn a_signed_out_login_pushes_nothing() {
    let w = World::new("log-signed-out");
    assert_eq!(w.store.sync().unwrap().active, None);
    assert!(w.sink.events().is_empty());
    assert!(!file(&w).exists());
}

#[test]
fn a_switch_that_is_refused_pushes_only_what_was_already_true() {
    let w = World::new("log-refused");
    w.signed_in(1, 1);
    w.answers_profile(1);
    assert!(w.store.switch_to("a9a9:0909").is_err());
    assert_eq!(ids(&w), [id(1)]);
}

#[test]
fn a_limit_switch_carries_its_window_and_percent() {
    let w = World::new("log-limit");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.saved(2, 1, LATER);
    let why = SwitchWhy::limit(
        Some(notchling_events::kinds::choices::LimitWindow::FiveHour),
        Some(96.4),
    );
    w.store.switch_for(&id(2), &why).unwrap();
    let last = switched(&w.sink).pop().unwrap();
    assert_eq!(text(&last, "cause").as_deref(), Some("limit_auto"));
    assert_eq!(text(&last, "window").as_deref(), Some("five_hour"));
    assert_eq!(text(&last, "used_pct").as_deref(), Some("96"));
}

#[test]
fn adding_and_removing_a_login_push_account_login() {
    let w = World::new("log-login");
    w.saved(2, 1, LATER);
    w.store.remove(&id(2)).unwrap();
    let logins = of(&w.sink, "account.login");
    assert_eq!(logins.len(), 1);
    assert_eq!(text(&logins[0], "action").as_deref(), Some("removed"));
    assert_eq!(text(&logins[0], "account"), Some(id(2)));
    assert_eq!(text(&logins[0], "result").as_deref(), Some("done"));
}
