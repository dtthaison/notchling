use super::*;
use crate::sessions::testutil::TempDir;
use crate::store;
use serde_json::json;

pub(super) fn app_applied(mut asked: New) -> New {
    asked.applied_by = AppliedBy::App;
    asked
}

pub(super) fn new(session: &str, target: &str) -> New {
    New {
        kind: SETTING.to_string(),
        target: target.to_string(),
        change: json!({"key": target, "before": "a", "after": "b"}),
        facts: json!([{"line": "consent-setting-change", "args": {"key": target}}]),
        parts: None,
        reason: Some("because".to_string()),
        session: Some(session.to_string()),
        tool: "claude".to_string(),
        verified: true,
        root: Some("Terminal".to_string()),
        folder: None,
        organization: None,
        pid: 7,
        pid_start: Some(70),
        idem_key: None,
        request: json!({"op": "settings.set", "key": target, "value": "b"}),
        answer_on: AnswerOn::MacOrPhone,
        applied_by: AppliedBy::Daemon,
        expires_in_ms: DEFAULT_EXPIRY_MS,
    }
}

#[test]
fn a_request_is_saved_waiting_and_read_back() {
    let store = ConsentStore::in_memory_for_tests();
    let row = store.insert(&new("s1", "k1"), 1_000).unwrap();
    let row = row.row();
    assert!(row.id.starts_with('c') && row.id.len() == 33);
    assert_eq!(row.state, State::Waiting);
    assert_eq!(row.expires_ms, 1_000 + DEFAULT_EXPIRY_MS);
    assert_eq!(row.reason.as_deref(), Some("because"));
    assert_eq!(store.get(&row.id).unwrap().unwrap(), *row);
}

#[test]
fn the_first_answer_wins_and_a_deny_clears_the_reason() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store
        .insert(&new("s1", "k1"), 1_000)
        .unwrap()
        .row()
        .id
        .clone();
    let first = store
        .answer(&id, State::Denied, "phone", None, 2_000)
        .unwrap();
    let Taken::Won(row) = first else {
        panic!("first answer should win")
    };
    assert_eq!(
        (row.state, row.by.as_deref()),
        (State::Denied, Some("phone"))
    );
    assert_eq!(
        row.reason, None,
        "the agent's text goes when the request closes"
    );
    let second = store
        .answer(&id, State::Applying, "mac", None, 3_000)
        .unwrap();
    assert!(matches!(second, Taken::Lost(r) if r.state == State::Denied));
}

#[test]
fn an_answer_after_the_end_is_refused() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store.insert(&new("s1", "k1"), 0).unwrap().row().id.clone();
    let late = store
        .answer(&id, State::Applying, "mac", None, DEFAULT_EXPIRY_MS + 1)
        .unwrap();
    assert!(matches!(late, Taken::Lost(r) if r.state == State::Waiting));
    let due = store.expire_due(DEFAULT_EXPIRY_MS + 1).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].state, State::Expired);
    assert_eq!(due[0].reason, None);
}

#[test]
fn opening_the_sheet_moves_the_end_to_ten_minutes_out_once() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store
        .insert(&app_applied(new("s1", "k1")), 0)
        .unwrap()
        .row()
        .id
        .clone();
    let now = DEFAULT_EXPIRY_MS - 60_000;
    let row = store.open_sheet(&id, now).unwrap().unwrap();
    assert_eq!(row.state, State::Opened);
    assert_eq!(row.expires_ms, now + OPENED_MIN_MS);
    let again = store.open_sheet(&id, now + 1_000).unwrap().unwrap();
    assert_eq!(again.expires_ms, now + OPENED_MIN_MS, "moved once");
}

#[test]
fn a_restart_keeps_waiting_returns_opened_to_waiting_and_withdraws_applying() {
    let store = ConsentStore::in_memory_for_tests();
    let waiting = store.insert(&new("s1", "a"), 1).unwrap().row().id.clone();
    let opened = store
        .insert(&app_applied(new("s1", "b")), 2)
        .unwrap()
        .row()
        .id
        .clone();
    let applying = store.insert(&new("s1", "c"), 3).unwrap().row().id.clone();
    store.open_sheet(&opened, 4).unwrap();
    store
        .answer(&applying, State::Applying, "mac", None, 5)
        .unwrap();
    let restarted = store.restart(10).unwrap();
    let (live, withdrawn) = (restarted.waiting, restarted.withdrawn);
    let ids: Vec<&str> = live.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, [waiting.as_str(), opened.as_str()]);
    assert!(live.iter().all(|r| r.state == State::Waiting));
    assert_eq!(withdrawn.len(), 1);
    assert_eq!(withdrawn[0].state, State::Withdrawn);
}

#[test]
fn the_same_key_returns_the_same_request_in_any_state_and_another_value_is_refused() {
    let store = ConsentStore::in_memory_for_tests();
    let mut asked = new("s1", "k1");
    asked.idem_key = Some("key-1".to_string());
    let first = store.insert(&asked, 1_000).unwrap();
    let id = first.row().id.clone();
    store
        .answer(&id, State::Denied, "mac", None, 2_000)
        .unwrap();
    let again = store.insert(&asked, 3_000).unwrap();
    assert!(matches!(&again, Opened::Same(r) if r.id == id && r.state == State::Denied));
    let mut other = asked.clone();
    other.request = json!({"op": "settings.set", "key": "k1", "value": "z"});
    assert_eq!(store.insert(&other, 3_000), Err(Refusal::KeyReused));
    let mut elsewhere = asked.clone();
    elsewhere.session = Some("s2".to_string());
    assert!(matches!(
        store.insert(&elsewhere, 3_000).unwrap(),
        Opened::Fresh(_)
    ));
    assert!(matches!(
        store.insert(&asked, 1_000 + IDEM_MS + 1).unwrap(),
        Opened::Fresh(_)
    ));
}

#[test]
fn a_closed_request_is_deleted_a_week_after_it_closed() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store.insert(&new("s1", "k1"), 0).unwrap().row().id.clone();
    store.answer(&id, State::Denied, "mac", None, 1).unwrap();
    store.expire_due(KEEP_DAYS_MS - 1).unwrap();
    assert!(store.get(&id).unwrap().is_some());
    store.expire_due(KEEP_DAYS_MS + 2).unwrap();
    assert!(store.get(&id).unwrap().is_none());
}

const KEEP_DAYS_MS: i64 = 7 * 24 * 3_600_000;

#[test]
fn a_database_from_before_the_table_gets_it_and_an_older_build_reads_the_newer_one() {
    let tmp = TempDir::new("consent-old");
    let dir = tmp.root().to_path_buf();
    {
        let db = rusqlite::Connection::open(store::db_path(&dir)).unwrap();
        db.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO meta (key, value) VALUES ('kept', 'yes');
             PRAGMA user_version = 9;",
        )
        .unwrap();
    }
    let store = ConsentStore::open(&dir).unwrap();
    let id = store.insert(&new("s1", "k1"), 1).unwrap().row().id.clone();
    drop(store);
    let db = store::open(&dir).unwrap();
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 11);
    assert_eq!(store::meta(&db, "kept").unwrap().as_deref(), Some("yes"));
    drop(db);
    let reopened = ConsentStore::open(&dir).unwrap();
    assert!(reopened.get(&id).unwrap().is_some());
    assert_eq!(
        crate::store_class::table_class("consents"),
        Some(crate::store_class::Class::Node)
    );
}

#[test]
fn a_retry_after_the_change_was_applied_returns_the_original() {
    let store = ConsentStore::in_memory_for_tests();
    let mut asked = new("s1", "k1");
    asked.idem_key = Some("key-1".to_string());
    let id = store.insert(&asked, 1_000).unwrap().row().id.clone();
    store
        .answer(&id, State::Applying, "mac", None, 2_000)
        .unwrap();
    store.finish(&id, State::Applied, None, 3_000).unwrap();
    let mut retry = asked.clone();
    retry.change = json!({"key": "k1", "before": "b", "after": "b"});
    let again = store.insert(&retry, 4_000).unwrap();
    assert!(matches!(&again, Opened::Same(r) if r.id == id && r.state == State::Applied));
}

#[test]
fn the_extension_is_used_once_even_across_a_restart() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store
        .insert(&app_applied(new("s1", "k1")), 0)
        .unwrap()
        .row()
        .id
        .clone();
    let first = store.open_sheet(&id, 600_000).unwrap().unwrap();
    assert_eq!(first.expires_ms, 600_000 + OPENED_MIN_MS);
    assert!(first.extended);
    store.restart(601_000).unwrap();
    let back = store.get(&id).unwrap().unwrap();
    assert_eq!((back.state, back.extended), (State::Waiting, true));
    let again = store.open_sheet(&id, 800_000).unwrap().unwrap();
    assert_eq!(again.state, State::Opened);
    assert_eq!(again.expires_ms, first.expires_ms, "no second extension");
}

#[test]
fn the_sheet_does_not_extend_a_daemon_applied_request() {
    let store = ConsentStore::in_memory_for_tests();
    let row = store.insert(&new("s1", "k1"), 0).unwrap().row().clone();
    let after = store.open_sheet(&row.id, 60_000).unwrap().unwrap();
    assert_eq!(
        (after.state, after.expires_ms),
        (State::Waiting, row.expires_ms)
    );
    assert!(!after.extended);
}

#[test]
fn a_restart_expires_what_ended_while_the_daemon_was_down() {
    let store = ConsentStore::in_memory_for_tests();
    let late = store.insert(&new("s1", "a"), 0).unwrap().row().id.clone();
    let live = store
        .insert(&new("s1", "b"), DEFAULT_EXPIRY_MS)
        .unwrap()
        .row()
        .id
        .clone();
    let restarted = store.restart(DEFAULT_EXPIRY_MS + 1).unwrap();
    let ids: Vec<&str> = restarted.waiting.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, [live.as_str()]);
    assert_eq!(restarted.expired.len(), 1);
    assert_eq!(restarted.expired[0].id, late);
    assert_eq!(restarted.expired[0].state, State::Expired);
}

#[test]
fn a_finish_after_the_end_records_what_the_action_did() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store.insert(&new("s1", "k1"), 0).unwrap().row().id.clone();
    assert!(store
        .finish(&id, State::Applied, None, DEFAULT_EXPIRY_MS + 1)
        .unwrap()
        .is_none());
    store
        .answer(&id, State::Applying, "mac", None, 1_000)
        .unwrap();
    let done = store
        .finish(&id, State::Applied, None, DEFAULT_EXPIRY_MS + 1)
        .unwrap()
        .unwrap();
    assert_eq!(done.state, State::Applied);
}

const BEFORE_THE_REQUEST_COLUMN: &str = "CREATE TABLE consents (
    n INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, kind TEXT NOT NULL,
    target TEXT NOT NULL, change TEXT NOT NULL, facts TEXT NOT NULL, parts TEXT,
    parts_allowed TEXT, reason TEXT, session TEXT, tool TEXT NOT NULL,
    verified INTEGER NOT NULL DEFAULT 0, root TEXT, folder TEXT, organization TEXT,
    pid INTEGER NOT NULL, pid_start INTEGER, idem_key TEXT, state TEXT NOT NULL,
    answer_on TEXT NOT NULL, applied_by TEXT NOT NULL, created_ms INTEGER NOT NULL,
    expires_ms INTEGER NOT NULL, answered_ms INTEGER, by TEXT, used_ms INTEGER,
    used_op_id TEXT, result TEXT, closed_ms INTEGER);
INSERT INTO consents (id, kind, target, change, facts, session, tool, verified, root, pid,
    idem_key, state, answer_on, applied_by, created_ms, expires_ms)
VALUES ('c-old-setting', 'setting', 'k1', '{\"key\":\"k1\",\"before\":\"a\",\"after\":\"b\"}',
    '[]', 's1', 'claude', 1, 'Terminal', 7, 'key-1', 'waiting', 'mac_or_phone', 'daemon',
    1000, 901000),
    ('c-old-secret', 'secret_request', 'npm',
    '{\"name\":\"npm\",\"request\":{\"name\":\"npm\",\"url\":\"https://npmjs.com\"}}',
    '[]', 's1', 'claude', 1, 'Terminal', 7, 'key-2', 'waiting', 'mac', 'app', 1000, 901000);";

#[test]
fn a_retry_of_a_request_saved_before_the_request_column_returns_it() {
    let tmp = TempDir::new("consent-prefix");
    let dir = tmp.root().to_path_buf();
    {
        let db = rusqlite::Connection::open(store::db_path(&dir)).unwrap();
        store::migrate(&db).unwrap();
        db.execute_batch(&format!("DROP TABLE consents; {BEFORE_THE_REQUEST_COLUMN}"))
            .unwrap();
    }
    let store = ConsentStore::open(&dir).unwrap();
    let mut setting = new("s1", "k1");
    setting.idem_key = Some("key-1".to_string());
    let again = store.insert(&setting, 2_000).unwrap();
    assert!(
        matches!(&again, Opened::Same(r) if r.id == "c-old-setting"),
        "{again:?}"
    );
    let mut secret = app_applied(new("s1", "npm"));
    secret.kind = "secret_request".to_string();
    secret.idem_key = Some("key-2".to_string());
    secret.request = json!({"op": "vault.request",
        "request": {"name": "npm", "url": "https://npmjs.com"}});
    let again = store.insert(&secret, 2_000).unwrap();
    assert!(
        matches!(&again, Opened::Same(r) if r.id == "c-old-secret"),
        "{again:?}"
    );
}

#[test]
fn a_row_an_older_build_wrote_with_no_request_still_matches_its_retry() {
    let store = ConsentStore::in_memory_for_tests();
    let mut asked = new("s1", "k1");
    asked.idem_key = Some("key-1".to_string());
    let id = store.insert(&asked, 1_000).unwrap().row().id.clone();
    store.run_sql_for_tests("UPDATE consents SET request = 'null'");
    let again = store.insert(&asked, 2_000).unwrap();
    assert!(matches!(&again, Opened::Same(r) if r.id == id), "{again:?}");
}

#[test]
fn no_randomness_refuses_and_saves_nothing() {
    let store = ConsentStore::in_memory_for_tests();
    let refused = store.insert_with(&new("s1", "k1"), 1, &|| None);
    assert!(matches!(refused, Err(Refusal::Db(_))), "{refused:?}");
    assert!(store.all().unwrap().is_empty());
}

#[test]
fn an_unavailable_store_refuses_everything() {
    let store = ConsentStore::unavailable();
    assert!(matches!(
        store.insert(&new("s1", "k1"), 1),
        Err(Refusal::Db(_))
    ));
    assert!(store.get("c1").is_err());
    assert!(store.all().is_err());
}

#[test]
fn a_version_nine_database_opens_as_ten_and_keeps_every_row() {
    let tmp = TempDir::new("consent-v9");
    let dir = tmp.root().to_path_buf();
    {
        let db = rusqlite::Connection::open(store::db_path(&dir)).unwrap();
        store::migrate(&db).unwrap();
        db.execute_batch(
            "DROP TABLE consents;
             INSERT INTO meta (key, value) VALUES ('kept', 'yes');
             INSERT INTO settings (key, value) VALUES ('theme', '\"dark\"');
             INSERT INTO drafts (row_id, text, updated_ms) VALUES ('r1', 'draft', 5);
             INSERT INTO docs (name, value) VALUES ('d1', '{}');
             INSERT INTO inbox (id, value, updated_ms) VALUES ('i1', '{}', 6);
             INSERT INTO outbox (id, session, tool, kind, text, via, state, queued_ms, updated_ms)
                 VALUES ('o1', 's', 'claude', 'prompt', 'kept', 'chat', 'queued', 1, 1);
             PRAGMA user_version = 9;",
        )
        .unwrap();
    }
    let consents = ConsentStore::open(&dir).unwrap();
    let id = consents
        .insert(&new("s1", "k1"), 1)
        .unwrap()
        .row()
        .id
        .clone();
    drop(consents);
    let db = store::open(&dir).unwrap();
    let count = |table: &str| -> i64 {
        db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 11);
    for table in [
        "meta", "settings", "drafts", "docs", "inbox", "outbox", "consents",
    ] {
        assert_eq!(count(table), 1, "{table}");
    }
    let reopened = ConsentStore::open(&dir).unwrap();
    let row = reopened.get(&id).unwrap().unwrap();
    assert_eq!(
        (row.request["op"].as_str(), row.extended),
        (Some("settings.set"), false)
    );
}

fn unverified(pid: u32, pid_start: Option<u64>, root: &str) -> New {
    let mut asked = new("none", "k1");
    asked.verified = false;
    asked.session = None;
    asked.root = Some(root.to_string());
    asked.pid = pid;
    asked.pid_start = pid_start;
    asked.expires_in_ms = 60_000;
    asked.idem_key = Some("key-1".to_string());
    asked
}

#[test]
fn an_unverified_key_belongs_to_its_own_process_and_not_to_the_shared_bucket() {
    let store = ConsentStore::in_memory_for_tests();
    let first = unverified(7, Some(70), "unrooted");
    let id = store.insert(&first, 1_000).unwrap().row().id.clone();
    store
        .answer(&id, State::Applying, "mac", None, 1_500)
        .unwrap();
    store.finish(&id, State::Applied, None, 1_600).unwrap();
    let again = store.insert(&first, 2_000).unwrap();
    assert!(matches!(&again, Opened::Same(r) if r.id == id), "{again:?}");
    let mut changed = first.clone();
    changed.request = json!({"op": "settings.set", "key": "k1", "value": "z"});
    assert_eq!(store.insert(&changed, 2_000), Err(Refusal::KeyReused));
    for other in [
        unverified(8, Some(70), "unrooted"),
        unverified(7, Some(71), "unrooted"),
    ] {
        let mut other = other;
        other.request = json!({"op": "settings.set", "key": "k1", "value": "z"});
        let opened = store.insert(&other, 2_000).unwrap();
        assert!(
            matches!(&opened, Opened::Fresh(r) if r.id != id),
            "{opened:?}"
        );
        let other_id = opened.row().id.clone();
        store
            .answer(&other_id, State::Applying, "mac", None, 2_100)
            .unwrap();
        store
            .finish(&other_id, State::Applied, None, 2_200)
            .unwrap();
    }
}

#[test]
fn an_unverified_caller_with_no_start_time_owns_no_key() {
    let store = ConsentStore::in_memory_for_tests();
    let first = unverified(7, None, "unrooted");
    let id = store.insert(&first, 1_000).unwrap().row().id.clone();
    store
        .answer(&id, State::Applying, "mac", None, 1_500)
        .unwrap();
    store.finish(&id, State::Applied, None, 1_600).unwrap();
    let again = store.insert(&first, 2_000).unwrap();
    assert!(
        matches!(&again, Opened::Fresh(r) if r.id != id),
        "{again:?}"
    );
}

#[test]
fn a_session_key_is_not_matched_by_an_unverified_caller_in_the_same_process() {
    let store = ConsentStore::in_memory_for_tests();
    let mut session = new("s1", "k1");
    session.idem_key = Some("key-1".to_string());
    let id = store.insert(&session, 1_000).unwrap().row().id.clone();
    let opened = store
        .insert(&unverified(7, Some(70), "unrooted"), 2_000)
        .unwrap();
    assert!(
        matches!(&opened, Opened::Fresh(r) if r.id != id),
        "{opened:?}"
    );
}

#[test]
fn a_row_written_under_an_old_root_still_matches_its_own_process() {
    let store = ConsentStore::in_memory_for_tests();
    let old = unverified(7, Some(70), "/bin/sh:70");
    let id = store.insert(&old, 1_000).unwrap().row().id.clone();
    let again = store
        .insert(&unverified(7, Some(70), "unrooted"), 2_000)
        .unwrap();
    assert!(matches!(&again, Opened::Same(r) if r.id == id), "{again:?}");
}

fn with_message(session: &str, target: &str) -> New {
    let mut asked = new(session, target);
    asked.change = json!({"action": "choose", "name": "n", "message": "secret text",
        "question": {"items": [{"question": "q"}], "answers": [[1]]}});
    asked
}

fn message_of(store: &ConsentStore, id: &str) -> Option<serde_json::Value> {
    let change = store.get(id).unwrap().unwrap().change;
    let message = change.get("message").cloned();
    assert_eq!(
        change.get("question").is_some(),
        message.is_some(),
        "the question goes with the message"
    );
    message
}

#[test]
fn the_message_goes_when_the_request_closes_however_it_closes() {
    let store = ConsentStore::in_memory_for_tests();
    let open = |target: &str| {
        store
            .insert(&with_message("s1", target), 1_000)
            .unwrap()
            .row()
            .id
            .clone()
    };
    let denied = open("a");
    assert_eq!(message_of(&store, &denied), Some(json!("secret text")));
    store
        .answer(&denied, State::Denied, "mac", None, 2_000)
        .unwrap();
    assert_eq!(message_of(&store, &denied), None);

    let cancelled = open("b");
    store
        .answer(&cancelled, State::Cancelled, "mac", None, 2_000)
        .unwrap();
    assert_eq!(message_of(&store, &cancelled), None);

    let applied = open("c");
    store
        .answer(&applied, State::Applying, "mac", None, 2_000)
        .unwrap();
    assert_eq!(
        message_of(&store, &applied),
        Some(json!("secret text")),
        "the apply still reads it"
    );
    store.finish(&applied, State::Applied, None, 3_000).unwrap();
    assert_eq!(message_of(&store, &applied), None);

    let failed = open("d");
    store
        .answer(&failed, State::Applying, "mac", None, 2_000)
        .unwrap();
    store.finish(&failed, State::Failed, None, 3_000).unwrap();
    assert_eq!(message_of(&store, &failed), None);

    let late = open("e");
    store.expire_due(DEFAULT_EXPIRY_MS + 5_000).unwrap();
    assert_eq!(message_of(&store, &late), None);
    assert_eq!(
        store.get(&late).unwrap().unwrap().change["action"],
        json!("choose")
    );
}

#[test]
fn a_restart_wipes_the_message_of_what_it_closes() {
    let store = ConsentStore::in_memory_for_tests();
    let applying = store
        .insert(&with_message("s1", "a"), 1_000)
        .unwrap()
        .row()
        .id
        .clone();
    store
        .answer(&applying, State::Applying, "mac", None, 2_000)
        .unwrap();
    let late = store
        .insert(&with_message("s1", "b"), 1_000)
        .unwrap()
        .row()
        .id
        .clone();
    store.restart(DEFAULT_EXPIRY_MS + 5_000).unwrap();
    assert_eq!(message_of(&store, &applying), None);
    assert_eq!(message_of(&store, &late), None);
}

#[test]
fn a_used_request_goes_back_to_the_state_it_was_taken_from_and_can_be_used_again() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store
        .insert(&app_applied(new("s1", "k1")), 0)
        .unwrap()
        .row()
        .id
        .clone();
    store.open_sheet(&id, 1_000).unwrap();
    let used = store.used(&id, "op-1", "mac", 2_000).unwrap().unwrap();
    assert_eq!(used.state, State::Applying);
    let back = store.reopen(&id, State::Opened).unwrap().unwrap();
    assert_eq!(back.state, State::Opened);
    assert_eq!(
        (back.by, back.answered_ms, back.used_ms, back.used_op_id),
        (None, None, None, None)
    );
    assert!(store.used(&id, "op-2", "mac", 3_000).unwrap().is_some());
}

#[test]
fn a_request_put_back_forgets_the_parts_it_was_taken_with() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store
        .insert(&app_applied(new("s1", "k1")), 0)
        .unwrap()
        .row()
        .id
        .clone();
    store.open_sheet(&id, 1_000).unwrap();
    let kept = json!(["hooks"]);
    let used = store
        .used_keeping(&id, "op-1", "mac", Some(&kept), 2_000)
        .unwrap()
        .unwrap();
    assert_eq!(used.parts_allowed, Some(kept));
    let back = store.reopen(&id, State::Opened).unwrap().unwrap();
    assert_eq!(back.parts_allowed, None);
    assert_eq!(store.get(&id).unwrap().unwrap().parts_allowed, None);
    let again = store
        .used_keeping(&id, "op-2", "mac", None, 3_000)
        .unwrap()
        .unwrap();
    assert_eq!(again.parts_allowed, None);
}

#[test]
fn only_a_request_in_use_is_reopened_and_only_to_an_open_state() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store
        .insert(&app_applied(new("s1", "k1")), 0)
        .unwrap()
        .row()
        .id
        .clone();
    assert!(store.reopen(&id, State::Waiting).unwrap().is_none());
    store.used(&id, "op-1", "mac", 2_000).unwrap();
    assert!(store.reopen(&id, State::Applied).unwrap().is_none());
    assert_eq!(store.get(&id).unwrap().unwrap().state, State::Applying);
    store
        .finish(&id, State::Applied, None, 3_000)
        .unwrap()
        .unwrap();
    assert!(store.reopen(&id, State::Waiting).unwrap().is_none());
    assert_eq!(store.get(&id).unwrap().unwrap().state, State::Applied);
}

#[test]
fn a_database_saved_before_the_note_column_gets_it_and_old_rows_read_none() {
    let tmp = TempDir::new("consent-note-column");
    let dir = tmp.root().to_path_buf();
    {
        let db = store::open(&dir).unwrap();
        db.execute_batch("ALTER TABLE consents DROP COLUMN note")
            .unwrap();
    }
    let store = ConsentStore::open(&dir).unwrap();
    let row = store.insert(&new("s1", "k1"), 1_000).unwrap();
    assert_eq!(row.row().note, None);
}
