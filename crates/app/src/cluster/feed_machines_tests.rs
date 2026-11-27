use notchling_local_api::{topics, Event};
use serde_json::json;

use super::*;

fn event(name: &str) -> Event {
    Event {
        event: name.to_owned(),
        data: json!({}),
    }
}

#[test]
fn a_machines_changed_event_redraws_the_cluster_views() {
    let shared = Shared::new();
    let before = shared.generation.load(Ordering::SeqCst);
    shared.apply(&event(topics::MACHINES_CHANGED));
    assert_eq!(shared.generation.load(Ordering::SeqCst), before + 1);
    shared.apply(&event("something.else"));
    assert_eq!(shared.generation.load(Ordering::SeqCst), before + 1);
}
