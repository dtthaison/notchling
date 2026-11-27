use super::tests::{at, build, opening};
use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::limits_ops_tests::TempDir;
use crate::watch_cli;
use notchling_engine::consent::{ConsentStore, SETTING};
use notchling_engine::inbox::ActionId;
use notchling_events::MemorySink;
use std::sync::atomic::AtomicI64;

const OTHERS_TARGET: &str = "planted-other-session-target";

struct Scoped {
    _tmp: TempDir,
    room: Arc<FakeRoom>,
    inbox: Arc<InboxOps>,
    consent: Arc<ConsentOps>,
}

impl Drop for Scoped {
    fn drop(&mut self) {
        self.consent.close();
    }
}

fn scoped(tag: &str) -> Scoped {
    let tmp = TempDir::new(&format!("consent-scoped-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let store = Arc::new(ConsentStore::in_memory_for_tests());
    let sink = Arc::new(MemorySink::new());
    let clock = Arc::new(AtomicI64::new(1_000_000));
    let (inbox, consent) = build(&tmp, &room, &store, &sink, &clock);
    Scoped {
        _tmp: tmp,
        room,
        inbox,
        consent,
    }
}

impl Scoped {
    fn open(&self, pid: u32, target: &str) -> Consent {
        self.consent
            .open(&at(pid), opening(SETTING, target, None))
            .unwrap()
            .row()
            .clone()
    }

    fn wait_for(&self, owner: &str, state: &str) -> Vec<(String, Value)> {
        for _ in 0..500 {
            let seen = self.room.scoped_to(owner, topics::MINE_CONSENTS);
            if seen.iter().any(|(_, d)| d["state"] == state) {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("no {state} event for {owner}");
    }
}

#[test]
fn a_request_is_told_to_its_own_session_and_to_no_other() {
    let r = scoped("owner");
    let mine = r.open(1, "limitMode");
    let theirs = r.open(2, OTHERS_TARGET);

    let a = r.room.scoped_to("s-a", topics::MINE_CONSENTS);
    let b = r.room.scoped_to("s-b", topics::MINE_CONSENTS);
    assert!(!a.is_empty() && !b.is_empty());
    for (event, data) in &a {
        assert_eq!(event, topics::CONSENT_CHANGED);
        assert_eq!(data["request"]["id"], json!(mine.id));
    }
    for (_, data) in &b {
        assert_eq!(data["request"]["id"], json!(theirs.id));
    }
    let all = serde_json::to_string(&a).unwrap();
    assert!(
        !all.contains(OTHERS_TARGET) && !all.contains(&theirs.id),
        "{all}"
    );
    let all = serde_json::to_string(&b).unwrap();
    assert!(
        !all.contains("limitMode") && !all.contains(&mine.id),
        "{all}"
    );
}

#[test]
fn a_request_is_never_published_to_everyone() {
    let r = scoped("public");
    r.open(1, "limitMode");
    r.open(2, OTHERS_TARGET);
    assert!(r.room.named(topics::CONSENT_CHANGED).is_empty());
    let everyone: Vec<String> = r
        .room
        .events
        .lock()
        .unwrap()
        .iter()
        .map(|(name, data)| format!("{name} {data}"))
        .collect();
    let text = everyone.join("\n");
    assert!(
        !text.contains("limitMode") && !text.contains(OTHERS_TARGET),
        "{text}"
    );
    let owners: Vec<String> = r
        .room
        .scoped
        .lock()
        .unwrap()
        .iter()
        .map(|(owner, topic, _, _)| format!("{owner}/{topic}"))
        .collect();
    assert!(owners
        .iter()
        .all(|o| o == "s-a/mine.consents" || o == "s-b/mine.consents"));
}

#[test]
fn a_shell_that_opens_a_request_has_no_session_to_tell() {
    let r = scoped("shell");
    let _ = r.consent.open(&at(99), opening(SETTING, "limitMode", None));
    assert!(r.room.scoped.lock().unwrap().is_empty());
}

#[test]
fn watch_until_applied_ends_when_the_request_is_applied() {
    let r = scoped("applied");
    let row = r.open(1, "limitMode");
    let args: Vec<String> = ["--until", "applied", "--topic", "mine.consents"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let parsed = watch_cli::parse(&args).unwrap();
    let waiting = r.wait_for("s-a", "waiting");
    assert!(waiting
        .iter()
        .all(|(event, data)| !watch_cli::reached(&parsed, data)
            && watch_cli::wanted(&parsed.topics, event)));

    let item = crate::consent_testkit::item_id_for(&r.inbox, &row.id);
    assert!(
        r.inbox
            .decide(&item, ActionId::Allow, "phone")
            .unwrap()
            .answered
    );
    let seen = r.wait_for("s-a", "applied");
    let ended = seen
        .iter()
        .position(|(_, data)| watch_cli::reached(&parsed, data))
        .expect("the applied event ends the watch");
    assert_eq!(seen[ended].1["state"], "applied");
    assert!(seen[..ended]
        .iter()
        .all(|(_, data)| data["state"] != "applied"));
    assert_eq!(seen[ended].1["request"]["id"], json!(row.id));
}
