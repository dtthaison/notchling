//! The switch feed's unit tests: its state, without a daemon.

use super::*;

fn event(name: &str, data: Value) -> Event {
    Event {
        event: name.to_owned(),
        data,
    }
}

fn shared() -> Shared {
    Shared {
        state: Mutex::new(State {
            switches: Switches::all_on(),
            following: true,
            ..State::default()
        }),
        stop: AtomicBool::new(false),
        restart: Arc::default(),
    }
}

#[test]
fn pending_vault_items_come_and_go_by_id_and_off_drops_them() {
    let s = shared();
    s.apply(&event(
        topics::VAULT_APPROVAL_PENDING,
        json!({"id": "a1", "names": ["github"]}),
    ));
    s.apply(&event(
        topics::VAULT_APPROVAL_PENDING,
        json!({"id": "a1", "names": ["github"]}),
    ));
    s.apply(&event(topics::VAULT_REQUEST_PENDING, json!({"id": "r1"})));
    s.apply(&event(
        topics::VAULT_REQUEST_PENDING,
        json!({"name": "no id"}),
    ));
    assert_eq!(s.state().approvals.len(), 1, "the same id once");
    assert_eq!(
        s.state().requests.len(),
        1,
        "an item without an id is nothing"
    );
    s.apply(&event(
        topics::VAULT_APPROVAL_RESOLVED,
        json!({"id": "a1", "decision": "once"}),
    ));
    assert!(s.state().approvals.is_empty());
    s.apply(&event(topics::VAULT_CHANGED, json!({"names": ["github"]})));
    assert_eq!(s.state().changes.vault, 1);
    // The vault switch going off ends the form still open.
    s.apply(&event(
        topics::SWITCH_CHANGED,
        json!({"switch": "vault", "on": false}),
    ));
    assert!(s.state().requests.is_empty());
    assert!(!s.state().switches.vault);
    assert_eq!(s.state().changes.vault, 2);
}

#[test]
fn the_areas_count_their_events_and_the_accounts_switch_moves_all_three() {
    let s = shared();
    s.apply(&event(topics::ACCOUNTS_CHANGED, json!({})));
    s.apply(&event(topics::ACCOUNTS_ADD_ENDED, json!({})));
    s.apply(&event(
        topics::PROVIDERS_CHANGED,
        json!({"provider": "codex"}),
    ));
    // The daemon publishes one of these for every providers.list, the app's own included: not a
    // reason to read again.
    s.apply(&event(
        topics::PROVIDERS_CHANGED,
        json!({"provider": null, "reason": "sync"}),
    ));
    s.apply(&event(
        "limits.notice",
        json!({"at": 1.0, "text": "me is at 91%"}),
    ));
    s.apply(&event("limits.notice", json!({"at": 2.0, "text": ""})));
    s.apply(&event("something.else", json!({})));
    let c = s.state().changes;
    assert_eq!((c.accounts, c.providers, c.limits, c.vault), (2, 1, 2, 0));
    assert_eq!(
        s.state().notices,
        vec![Notice {
            at: 1.0,
            text: "me is at 91%".into()
        }]
    );
    s.apply(&event(
        topics::SWITCH_CHANGED,
        json!({"switch": "accounts", "on": false}),
    ));
    let c = s.state().changes;
    assert_eq!((c.accounts, c.providers, c.limits), (3, 2, 3));
    // The same again changes nothing.
    s.apply(&event(
        topics::SWITCH_CHANGED,
        json!({"switch": "accounts", "on": false}),
    ));
    assert_eq!(s.state().changes.accounts, 3);
}

#[test]
fn notices_are_capped_and_a_lost_daemon_detaches_everything() {
    let s = shared();
    for i in 0..25 {
        s.apply(&event(
            "limits.notice",
            json!({"at": i as f64, "text": format!("n{i}")}),
        ));
    }
    let notices = s.state().notices.clone();
    assert_eq!(notices.len(), NOTICES_KEPT);
    assert_eq!(notices[0].text, "n5");
    s.apply(&event(topics::VAULT_APPROVAL_PENDING, json!({"id": "a1"})));
    s.lost();
    let st = s.state();
    assert!(!st.following && !st.switches.attached);
    assert!(st.approvals.is_empty());
}

#[test]
fn a_feed_with_no_daemon_shows_its_fixed_switches_and_cannot_choose() {
    let feed = SwitchFeed::fixed(Switches::all_on());
    assert!(!feed.is_live() && !feed.following());
    assert!(feed.switches().is_on(Switch::Vault));
    assert!(feed.approvals().is_empty() && feed.requests().is_empty());
    assert!(!feed.chosen(Switch::Vault));
    assert!(feed.choose(Switch::Vault, true).is_err());
    let none = SwitchFeed::none();
    assert_eq!(none.switches(), Switches::detached());
}

#[test]
fn the_choice_is_kept_in_the_apps_own_settings() {
    let scratch = crate::testutil::Scratch::new("switch-choice");
    let file = scratch.path().join("settings.json");
    let choice = SettingsChoice { file: file.clone() };
    assert!(!choice.get(Switch::Vault));
    choice.set(Switch::Vault, true).unwrap();
    assert!(SettingsChoice { file: file.clone() }.get(Switch::Vault));
    assert!(!choice.get(Switch::Accounts));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("vaultOn"), "{text}");
}
