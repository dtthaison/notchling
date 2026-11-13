use super::tests_support::*;
use super::*;
use crate::cost::account_log::{self, Entry};

const NOW_SECS: f64 = NOW as f64 / 1000.0;

fn entries(w: &World) -> Vec<Entry> {
    account_log::load(&w.tmp.path("data/account-log.json"))
}

fn ids(w: &World) -> Vec<String> {
    entries(w).into_iter().map(|e| e.id).collect()
}

fn raw(w: &World) -> String {
    std::fs::read_to_string(w.tmp.path("data/account-log.json")).unwrap_or_default()
}

#[test]
fn a_switch_logs_the_account_it_left_and_the_one_it_went_to() {
    let w = World::new("log-switch");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.saved(2, 1, LATER);
    w.store.switch_to(&id(2)).unwrap();
    assert_eq!(ids(&w), [id(1), id(2)]);
    assert!(entries(&w).iter().all(|e| e.at == NOW_SECS));
}

#[test]
fn cost_follows_the_log_after_a_switch() {
    let w = World::new("log-cost");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.saved(2, 1, LATER);
    w.store.sync().unwrap();
    w.clock
        .store(NOW + 60_000, std::sync::atomic::Ordering::SeqCst);
    w.store.switch_to(&id(2)).unwrap();
    let log = entries(&w);
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
    assert_eq!(entries(&w)[1].at, NOW_SECS + 60.0);
}

#[test]
fn the_same_account_is_logged_once_even_across_restarts() {
    let w = World::new("log-once");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.store.sync().unwrap();
    w.store.sync().unwrap();
    let text = raw(&w);
    let again = ClaudeAccounts::new(ClaudeEnv {
        keys: w.store.env().keys.clone(),
        undo_keys: w.store.env().undo_keys.clone(),
        http: w.store.env().http.clone(),
        now_ms: Arc::new(|| LATER),
        config_file: w.store.env().config_file.clone(),
        data_dir: w.store.env().data_dir.clone(),
        list_file: w.store.env().list_file.clone(),
        lanes: LaneStore::new(&w.tmp.path("home"), &w.tmp.path("data")),
    });
    again.sync().unwrap();
    assert_eq!(ids(&w), [id(1)]);
    assert_eq!(raw(&w), text);
}

#[test]
fn a_log_the_swift_app_wrote_keeps_its_bytes_and_gets_the_new_entry_after_them() {
    let w = World::new("log-swift");
    let old = r#"[{"at":814681048,"id":"a3a3:0303"},{"at":815978594.1160798,"id":"a4a4:0404"}]"#;
    w.tmp.write("data/account-log.json", old);
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.store.sync().unwrap();
    let text = raw(&w);
    assert!(text.starts_with(&old[..old.len() - 1]), "{text}");
    assert!(text.ends_with(r#","id":"a1a1:0101"}]"#), "{text}");
    assert_eq!(ids(&w), ["a3a3:0303", "a4a4:0404", "a1a1:0101"]);
}

#[test]
fn an_empty_or_missing_log_starts_a_list_and_an_unreadable_one_is_left_alone() {
    let w = World::new("log-shapes");
    w.tmp.write("data/account-log.json", "[]");
    w.signed_in(1, 1);
    w.answers_profile(1);
    w.store.sync().unwrap();
    assert_eq!(ids(&w), [id(1)]);

    let broken = World::new("log-broken");
    broken.tmp.write("data/account-log.json", "{not a list");
    broken.signed_in(1, 1);
    broken.answers_profile(1);
    assert_eq!(broken.store.sync().unwrap().active, Some(id(1)));
    assert_eq!(raw(&broken), "{not a list");
}

#[test]
fn a_signed_out_login_logs_nothing() {
    let w = World::new("log-signed-out");
    assert_eq!(w.store.sync().unwrap().active, None);
    assert!(!w.tmp.path("data/account-log.json").exists());
}

#[test]
fn a_switch_that_is_refused_logs_only_what_was_already_true() {
    let w = World::new("log-refused");
    w.signed_in(1, 1);
    w.answers_profile(1);
    assert!(w.store.switch_to("a9a9:0909").is_err());
    assert_eq!(ids(&w), [id(1)]);
}
