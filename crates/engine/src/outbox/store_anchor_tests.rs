use super::*;
use crate::sessions::testutil::TempDir;
use crate::store;
use crate::transcript::sent::Seen;

fn new(session: &str, text: &str) -> New {
    New {
        session: session.to_string(),
        tool: Tool::Claude,
        text: text.to_string(),
        options: None,
        via: Via::Chat,
        clear_box: false,
    }
}

#[test]
fn migrate_from_v5_adds_the_anchor_and_old_rows_load_without_one() {
    let tmp = TempDir::new("outbox-v5");
    let dir = tmp.path("data");
    std::fs::create_dir_all(&dir).unwrap();
    {
        let db = rusqlite::Connection::open(store::db_path(&dir)).unwrap();
        db.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE drafts (row_id TEXT PRIMARY KEY, text TEXT NOT NULL, updated_ms INTEGER NOT NULL);
             CREATE TABLE docs (name TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE inbox (id TEXT PRIMARY KEY, value TEXT NOT NULL, updated_ms INTEGER NOT NULL);
             CREATE TABLE outbox (
                 n INTEGER PRIMARY KEY AUTOINCREMENT,
                 id TEXT NOT NULL UNIQUE,
                 session TEXT NOT NULL,
                 tool TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 text TEXT NOT NULL,
                 options TEXT,
                 via TEXT NOT NULL,
                 state TEXT NOT NULL,
                 why TEXT,
                 clear_box INTEGER NOT NULL DEFAULT 0,
                 queued_ms INTEGER NOT NULL,
                 updated_ms INTEGER NOT NULL,
                 attempt_ms INTEGER,
                 seen_before INTEGER,
                 queued_before INTEGER,
                 tries INTEGER NOT NULL DEFAULT 0,
                 next_ms INTEGER,
                 noticed_ms INTEGER
             );
             CREATE TABLE asks (
                 n INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, key TEXT NOT NULL,
                 session TEXT NOT NULL, session_id TEXT, asker TEXT NOT NULL, agent_id TEXT,
                 label TEXT, agent_type TEXT, model TEXT, tool_use_id TEXT, folder TEXT,
                 branch TEXT, question TEXT NOT NULL, choices TEXT,
                 main_may_answer INTEGER NOT NULL DEFAULT 0, mode TEXT NOT NULL,
                 deadline_ms INTEGER, pid INTEGER NOT NULL, pid_start INTEGER, state TEXT NOT NULL,
                 answer TEXT, choice INTEGER, by TEXT, inbox_id TEXT, created_ms INTEGER NOT NULL,
                 updated_ms INTEGER NOT NULL, closed_ms INTEGER
             );
             INSERT INTO outbox (id, session, tool, kind, text, via, state, attempt_ms,
                     seen_before, queued_before, queued_ms, updated_ms)
                 VALUES ('o1', 's', 'claude', 'prompt', 'kept', 'chat', 'sending', 5, 1, 0, 1, 1);
             PRAGMA user_version = 5;",
        )
        .unwrap();
    }
    let outbox = OutboxStore::open(&dir).unwrap();
    let rows = outbox.list(None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "kept");
    assert_eq!(rows[0].seen_before, Some(1));
    assert_eq!(rows[0].anchor, None, "an old row has no anchor");
    assert_eq!(
        (
            rows[0].aside_at_ms,
            rows[0].aside_ms,
            rows[0].hold_ms,
            rows[0].joined
        ),
        (None, None, None, 0),
        "an old row has no aside plan, hold or join"
    );
    outbox
        .set_baseline(
            "o1",
            Seen::default(),
            Some(&Anchor {
                file: Some("/p/a.jsonl".into()),
                slack_ms: 2_000,
            }),
        )
        .unwrap();
    assert_eq!(
        outbox.get("o1").unwrap().unwrap().anchor,
        Some(Anchor {
            file: Some("/p/a.jsonl".into()),
            slack_ms: 2_000
        })
    );
    drop(outbox);
    let again = OutboxStore::open(&dir).unwrap();
    assert!(again.get("o1").unwrap().unwrap().anchor.is_some());
    let version: i64 = store::open(&dir)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 9);
}

#[test]
fn the_anchor_is_stored_with_the_baseline_and_a_take_or_a_requeue_clears_it() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "a"), 1).unwrap();
    assert_eq!(row.anchor, None);
    store.take(&row.id, 2).unwrap().unwrap();
    let here = Anchor {
        file: Some("/p/a.jsonl".into()),
        slack_ms: 2_000,
    };
    store
        .set_baseline(
            &row.id,
            Seen {
                enqueued: 1,
                taken: 3,
            },
            Some(&here),
        )
        .unwrap();
    let now = store.get(&row.id).unwrap().unwrap();
    assert_eq!((now.seen_before, now.queued_before), (Some(3), Some(1)));
    assert_eq!(now.anchor, Some(here.clone()));

    store
        .set_baseline(
            &row.id,
            Seen::default(),
            Some(&Anchor {
                file: None,
                slack_ms: 2_000,
            }),
        )
        .unwrap();
    assert_eq!(
        store.get(&row.id).unwrap().unwrap().anchor,
        Some(Anchor {
            file: None,
            slack_ms: 2_000
        }),
        "no file at the try is an anchor of its own"
    );

    store
        .set_state(
            &row.id,
            &[State::Sending],
            State::Unsure,
            Some(Why::Restart),
            None,
            3,
        )
        .unwrap();
    store.requeue(&row.id, &[State::Unsure], false, 4).unwrap();
    assert_eq!(store.get(&row.id).unwrap().unwrap().anchor, None);

    store.take(&row.id, 5).unwrap().unwrap();
    store
        .set_baseline(&row.id, Seen::default(), Some(&here))
        .unwrap();
    store.untake(&row.id, false, 0, 6).unwrap();
    assert_eq!(store.get(&row.id).unwrap().unwrap().anchor, None);

    store.take(&row.id, 7).unwrap().unwrap();
    store
        .set_baseline(&row.id, Seen::default(), Some(&here))
        .unwrap();
    store
        .set_state(
            &row.id,
            &[State::Sending],
            State::Waiting,
            Some(Why::Retry),
            Some(9),
            8,
        )
        .unwrap();
    store.take(&row.id, 10).unwrap().unwrap();
    assert_eq!(
        store.get(&row.id).unwrap().unwrap().anchor,
        None,
        "a new try starts with no anchor"
    );
}
