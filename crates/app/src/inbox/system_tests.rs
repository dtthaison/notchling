use super::*;
use notchling_platform::notify::RecordingNotifier;
use serde_json::{json, Value};

fn item(id: &str, state: &str, actions: &[&str]) -> Value {
    let actions: Vec<Value> = actions
        .iter()
        .map(|a| json!({"id": a, "label": a, "primary": false}))
        .collect();
    json!({
        "id": id,
        "kind": "secret_approval",
        "title": format!("Title {id}"),
        "detail": format!("Detail {id}"),
        "source": {},
        "created_ms": 1_000,
        "expires_ms": null,
        "actions": actions,
        "state": state,
        "answer": null,
        "answered_by": null,
        "seen": false,
        "open": null
    })
}

fn changed(item: Value) -> Value {
    json!({"event": "inbox.changed", "data": {"item": item}})
}

fn arrive(model: &mut InboxModel, notified: &mut HashSet<String>, wire: Value, n: &dyn Notifier) {
    let before = model.clone();
    model.apply(&wire);
    notify_new(&before, model, notified, n);
}

#[test]
fn actionable_item_notifies_once() {
    let notifier = RecordingNotifier::new();
    let mut model = InboxModel::default();
    let mut notified = HashSet::new();
    let wire = changed(item("a", "open", &["allow_once", "deny"]));
    arrive(&mut model, &mut notified, wire.clone(), &notifier);
    assert_eq!(notifier.sent(), [Notification::new("Title a", "Detail a")]);

    arrive(&mut model, &mut notified, wire.clone(), &notifier);
    let mut update = item("a", "open", &["allow_once", "deny"]);
    update["seen"] = json!(true);
    arrive(&mut model, &mut notified, changed(update), &notifier);
    assert_eq!(notifier.sent().len(), 1);

    model.clear();
    arrive(&mut model, &mut notified, wire, &notifier);
    assert_eq!(notifier.sent().len(), 1);
}

#[test]
fn notice_only_item_does_not_notify() {
    let notifier = RecordingNotifier::new();
    let mut model = InboxModel::default();
    let mut notified = HashSet::new();
    let wire = changed(item("n", "open", &["done"]));
    arrive(&mut model, &mut notified, wire, &notifier);
    let wire = changed(item("e", "open", &[]));
    arrive(&mut model, &mut notified, wire, &notifier);
    assert!(notifier.sent().is_empty());
}

#[test]
fn an_item_that_is_not_open_does_not_notify() {
    let notifier = RecordingNotifier::new();
    let mut model = InboxModel::default();
    let mut notified = HashSet::new();
    for (id, state) in [("a", "answered"), ("b", "expired"), ("c", "withdrawn")] {
        let wire = changed(item(id, state, &["deny"]));
        arrive(&mut model, &mut notified, wire, &notifier);
    }
    assert!(notifier.sent().is_empty());
}

#[test]
fn an_item_that_was_there_before_does_not_notify_when_it_changes() {
    let notifier = RecordingNotifier::new();
    let mut model = InboxModel::default();
    model.load(&json!({"items": [item("a", "open", &["deny"])]}));
    let mut notified = HashSet::new();
    let wire = changed(item("a", "open", &["deny", "cancel"]));
    arrive(&mut model, &mut notified, wire, &notifier);
    assert!(notifier.sent().is_empty());
}

#[test]
fn a_reseed_after_a_loss_does_not_notify_what_was_remembered() {
    let notifier = RecordingNotifier::new();
    let mut model = InboxModel::default();
    let mut notified = HashSet::new();
    let wire = changed(item("a", "open", &["deny"]));
    arrive(&mut model, &mut notified, wire, &notifier);
    assert_eq!(notifier.sent().len(), 1);

    model.clear();
    let reply = json!({"items": [item("a", "open", &["deny"]), item("b", "open", &["deny"])]});
    model.load(&reply);
    remember(&model, &mut notified);
    model.clear();
    for id in ["a", "b"] {
        let wire = changed(item(id, "open", &["deny"]));
        arrive(&mut model, &mut notified, wire, &notifier);
    }
    assert_eq!(notifier.sent().len(), 1);

    let wire = changed(item("c", "open", &["deny"]));
    arrive(&mut model, &mut notified, wire, &notifier);
    assert_eq!(notifier.sent().len(), 2);
}

#[test]
fn a_detached_notifier_hands_banners_to_the_inner_one_in_order() {
    let inner = Arc::new(RecordingNotifier::new());
    let detached = Detached::new(inner.clone());
    let first = Notification::new("t1", "b1");
    let second = Notification::new("t2", "b2");
    detached.notify(&first).expect("accepted");
    detached.notify(&second).expect("accepted");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while inner.sent().len() < 2 && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(inner.sent(), [first, second]);
}
