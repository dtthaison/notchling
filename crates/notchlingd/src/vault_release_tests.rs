use super::tests::{app, seed, setup, Setup};
use super::*;
use crate::event_assert::results;
use notchling_engine::cluster::grants::decode_env;
use std::sync::atomic::Ordering;

const TOKEN: &str = "fake-token-value-0001";

fn who() -> ReleaseFor {
    ReleaseFor {
        node_name: "worker".to_string(),
        job: "j-1".to_string(),
        command: "make deploy".to_string(),
        folder: "/w/app".to_string(),
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn failure(r: Result<Released, ApiError>) -> ApiError {
    match r {
        Err(e) => e,
        Ok(_) => panic!("expected a refusal"),
    }
}

fn pending_release(s: &Setup) -> (std::thread::JoinHandle<Result<Released, ApiError>>, Value) {
    s.room.follows.store(true, Ordering::SeqCst);
    let asked = || {
        let events = s.room.events.lock().unwrap();
        events
            .iter()
            .filter(|e| e.1 == topics::VAULT_APPROVAL_PENDING)
            .count()
    };
    let before = asked();
    let ops = s.ops.clone();
    let user = std::thread::spawn(move || ops.release(&who(), &names(&["gh"])));
    for _ in 0..500 {
        if asked() > before {
            let event = s.room.pending(topics::VAULT_APPROVAL_PENDING).unwrap();
            return (user, event);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("no approval asked");
}

fn answer(s: &Setup, event: &Value, decision: &str) {
    let args = json!({"id": event["id"], "decision": decision});
    s.ops
        .call(&app(), ops::VAULT_APPROVAL_ANSWER, &args)
        .unwrap();
}

#[test]
fn a_release_gives_the_env_and_leaves_the_audit_to_the_caller() {
    let s = setup("rel-plain", true);
    seed(&s, false);
    let got = s.ops.release(&who(), &names(&["GH", "gh"])).unwrap();
    assert_eq!(got.values.len(), 1);
    assert!(!got.asked);
    let (entry, value) = &got.values[0];
    assert_eq!(entry.id(), "gh");
    let env = decode_env(value).unwrap();
    assert_eq!(env["GH_TOKEN"].expose(), TOKEN);
    assert!(results(&s.sink, "vault.secret_used").is_empty());
    assert!(s.ops.note_released(&who(), "gh", &got));
    assert_eq!(results(&s.sink, "vault.secret_used"), ["given"]);
    assert!(!format!("{:?}", s.sink.events()).contains(TOKEN));
    s.sink.fail_commits(true);
    assert!(!s.ops.note_released(&who(), "gh", &got));
}

#[test]
fn a_lookup_reads_the_list_and_never_a_value() {
    let s = setup("rel-look", true);
    seed(&s, true);
    s.ops.store.forget();
    let reads = *s.item.reads.lock().unwrap();
    let got = s.ops.lookup(&names(&["GH", "reg", "ghost"])).unwrap();
    let said: Vec<(bool, bool)> = got.iter().map(|l| (l.exists, l.asks)).collect();
    assert_eq!(said, [(true, true), (true, false), (false, false)]);
    assert_eq!(*s.item.reads.lock().unwrap(), reads);
}

#[test]
fn while_the_vault_is_off_nothing_is_looked_up_or_released() {
    let s = setup("rel-off", false);
    let e = s.ops.lookup(&names(&["gh"])).unwrap_err();
    let released = failure(s.ops.release(&who(), &names(&["gh"])));
    assert_eq!(e.code, released.code);
    assert_eq!(*s.item.reads.lock().unwrap(), 0);
}

#[test]
fn a_missing_name_is_named_without_listing_what_is_saved() {
    let s = setup("rel-missing", true);
    seed(&s, false);
    let e = failure(s.ops.release(&who(), &names(&["ghost"])));
    assert!(e.message.contains("No secret named ghost"), "{}", e.message);
    assert!(!e.message.contains("reg"), "{}", e.message);
}

#[test]
fn ask_first_names_the_node_and_job_and_releases_after_once() {
    let s = setup("rel-once", true);
    seed(&s, true);
    let (user, event) = pending_release(&s);
    assert_eq!(event["names"], json!(["gh"]));
    assert_eq!(event["command"], "make deploy");
    assert_eq!(event["folder"], "/w/app");
    assert_eq!(event["verified"], json!({"node": "worker", "job": "j-1"}));
    assert!(!format!("{event}").contains(TOKEN));
    answer(&s, &event, "once");
    let got = user.join().unwrap().unwrap();
    assert!(got.asked);
    assert_eq!(got.values.len(), 1);
    assert_eq!(s.ops.store.entry("gh").unwrap().unwrap().ask, Some(true));
}

#[test]
fn a_denied_release_gives_nothing_and_is_audited_as_denied() {
    let s = setup("rel-deny", true);
    seed(&s, true);
    let (user, event) = pending_release(&s);
    answer(&s, &event, "deny");
    let e = failure(user.join().unwrap());
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::NeedsApproval, "The user didn't allow it.")
    );
    assert_eq!(results(&s.sink, "vault.secret_used"), ["denied"]);
    assert!(!format!("{:?}", s.sink.events()).contains(TOKEN));
}

#[test]
fn always_allow_stops_asking_for_that_secret() {
    let s = setup("rel-always", true);
    seed(&s, true);
    let (user, event) = pending_release(&s);
    answer(&s, &event, "always");
    assert!(user.join().unwrap().is_ok());
    assert_eq!(s.ops.store.entry("gh").unwrap().unwrap().ask, Some(false));
    let again = s.ops.release(&who(), &names(&["gh"])).unwrap();
    assert!(!again.asked);
}

#[test]
fn ask_first_with_no_app_reads_no_value() {
    let s = setup("rel-noapp", true);
    seed(&s, true);
    s.ops.store.forget();
    let reads = *s.item.reads.lock().unwrap();
    let e = failure(s.ops.release(&who(), &names(&["gh"])));
    assert_eq!(e.code, ErrorCode::NeedsApproval);
    assert_eq!(*s.item.reads.lock().unwrap(), reads);
    assert_eq!(results(&s.sink, "vault.secret_used"), ["no_app"]);
}
