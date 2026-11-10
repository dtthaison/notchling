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
    let kept = notchling_engine::settings::Settings::open(&file).unwrap();
    assert_eq!(kept.raw("vaultOn"), Some(&json!(true)));
}

#[test]
fn the_pending_list_read_on_connect_replaces_what_was_held_and_events_still_dedupe() {
    let s = shared();
    s.apply(&event(
        topics::VAULT_APPROVAL_PENDING,
        json!({"id": "gone"}),
    ));
    s.apply(&event(
        topics::VAULT_REQUEST_PENDING,
        json!({"id": "r-gone"}),
    ));
    let before = s.state().changes.vault;
    s.load_pending(&json!({
        "approvals": [{"id": "a2", "names": ["github"]}, {"id": "a2"}, {"id": "a3"}, {"no": "id"}],
        "requests": [{"id": "r1"}]
    }));
    let ids = |list: &Vec<Value>| -> Vec<String> {
        list.iter()
            .filter_map(|p| p["id"].as_str().map(str::to_owned))
            .collect()
    };
    {
        let st = s.state();
        assert_eq!(ids(&st.approvals), ["a2", "a3"], "oldest first, once each");
        assert_eq!(ids(&st.requests), ["r1"]);
        assert_eq!(st.approvals[0]["names"], json!(["github"]));
        assert_eq!(st.changes.vault, before + 1);
    }
    // An event for one already listed after the read adds nothing; a resolved one goes.
    s.apply(&event(topics::VAULT_APPROVAL_PENDING, json!({"id": "a3"})));
    s.apply(&event(topics::VAULT_APPROVAL_RESOLVED, json!({"id": "a2"})));
    assert_eq!(ids(&s.state().approvals), ["a3"]);
    // A reply this app can't read changes nothing.
    s.load_pending(&json!("odd"));
    assert_eq!(ids(&s.state().approvals), ["a3"]);
}

#[test]
fn an_accounts_event_with_a_moves_end_is_kept_for_the_view() {
    let s = shared();
    s.apply(&event(topics::ACCOUNTS_CHANGED, json!({"active": "a1"})));
    {
        let st = s.state();
        assert_eq!((st.changes.moves, st.last_move.clone()), (0, None));
    }
    let end = json!({"moved": ["job-1"], "errors": {"job-2": "Couldn't move it"}});
    s.apply(&event(topics::ACCOUNTS_CHANGED, end.clone()));
    {
        let st = s.state();
        assert_eq!((st.changes.moves, st.changes.accounts), (1, 2));
        assert_eq!(st.last_move, Some(end));
    }
    // The daemon's late answer: `accounts.done` for a Move All carries its result or its error.
    let result = json!({"moved": [], "errors": {"job-9": "Couldn't move it: stop failed"}});
    s.apply(&event(
        topics::ACCOUNTS_DONE,
        json!({"ticket": 4, "op": "accounts.move_all", "ok": true, "result": result}),
    ));
    let moves = s.state().changes.moves;
    assert_eq!((moves, s.state_move()), (2, Some(result)));
    s.apply(&event(
        topics::ACCOUNTS_DONE,
        json!({"ticket": 5, "op": "accounts.move_all", "ok": false, "error": "Acting is off."}),
    ));
    assert_eq!(
        s.state_move(),
        Some(json!({"moved": [], "errors": {"accounts": "Acting is off."}}))
    );
    // Another op's late answer only says the accounts moved.
    s.apply(&event(
        topics::ACCOUNTS_DONE,
        json!({"ticket": 6, "op": "accounts.switch", "ok": true, "result": {"moved": []}}),
    ));
    let st = s.state();
    assert_eq!((st.changes.moves, st.changes.accounts), (3, 5));
}

impl Shared {
    fn state_move(&self) -> Option<Value> {
        self.state().last_move.clone()
    }
}
