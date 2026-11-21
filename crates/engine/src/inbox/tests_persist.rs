use super::*;
use crate::sessions::testutil::TempDir;
use crate::store;

fn item(id: &str, created_ms: i64, state: State) -> Item {
    Item {
        id: id.to_string(),
        kind: Kind::Notice,
        title: format!("title {id}"),
        detail: String::new(),
        source: Source::default(),
        created_ms,
        expires_ms: None,
        actions: vec![Action {
            id: ActionId::Done,
            label: "Done".to_string(),
            primary: true,
        }],
        state,
        answer: None,
        answered_by: None,
        seen: false,
        open: None,
        closed_ms: None,
    }
}

fn ids(book: &Book) -> Vec<String> {
    book.items().into_iter().map(|item| item.id).collect()
}

#[test]
fn round_trips_items() {
    let tmp = TempDir::new("inbox-round");
    let dir = tmp.path("data");
    let mut a = item("a", 1, State::Answered);
    a.answer = Some(ActionId::Done);
    a.answered_by = Some("mac".to_string());
    a.closed_ms = Some(5);
    let mut b = item("b", 2, State::Expired);
    b.closed_ms = Some(6);
    b.open = Some(serde_json::json!({"session": "s1"}));
    let c = item("c", 3, State::Expired);
    save(&dir, &Change::Raised(a.clone())).unwrap();
    save(&dir, &Change::Updated(b.clone())).unwrap();
    save(&dir, &Change::Raised(c)).unwrap();
    save(&dir, &Change::Removed("c".to_string())).unwrap();
    let book = load(&dir, 100).unwrap();
    assert_eq!(book.items(), vec![b, a]);
}

#[test]
fn saving_again_replaces_the_row() {
    let tmp = TempDir::new("inbox-replace");
    let dir = tmp.path("data");
    let mut a = item("a", 1, State::Expired);
    save(&dir, &Change::Raised(a.clone())).unwrap();
    a.seen = true;
    save(&dir, &Change::Updated(a.clone())).unwrap();
    let book = load(&dir, 100).unwrap();
    assert_eq!(book.items(), vec![a]);
}

#[test]
fn open_items_from_a_previous_run_load_as_withdrawn() {
    let tmp = TempDir::new("inbox-open");
    let dir = tmp.path("data");
    save(&dir, &Change::Raised(item("a", 1, State::Open))).unwrap();
    let mut done = item("b", 2, State::Answered);
    done.closed_ms = Some(7);
    save(&dir, &Change::Raised(done.clone())).unwrap();
    let book = load(&dir, 500).unwrap();
    let items = book.items();
    assert_eq!(items[0], done);
    assert_eq!(items[1].state, State::Withdrawn);
    assert_eq!(items[1].closed_ms, Some(500));
    assert_eq!(book.unread(), 0);
}

#[test]
fn missing_db_loads_empty_and_makes_nothing() {
    let tmp = TempDir::new("inbox-missing");
    let dir = tmp.path("data");
    let book = load(&dir, 1).unwrap();
    assert!(ids(&book).is_empty());
    assert!(!dir.exists());
    save(&dir, &Change::Removed("x".to_string())).unwrap();
    assert!(!dir.exists());
}

#[test]
fn an_existing_database_without_the_table_gains_it() {
    let tmp = TempDir::new("inbox-migrate");
    let dir = tmp.path("data");
    store::put_draft(&dir, "row", "text", 1).unwrap();
    {
        let db = store::open(&dir).unwrap();
        db.execute_batch("DROP TABLE inbox; PRAGMA user_version = 2;")
            .unwrap();
    }
    save(&dir, &Change::Raised(item("a", 1, State::Expired))).unwrap();
    assert_eq!(ids(&load(&dir, 1).unwrap()), vec!["a".to_string()]);
}

#[test]
fn a_row_that_cannot_be_read_is_skipped() {
    let tmp = TempDir::new("inbox-bad");
    let dir = tmp.path("data");
    save(&dir, &Change::Raised(item("a", 1, State::Expired))).unwrap();
    store::open(&dir)
        .unwrap()
        .execute(
            "INSERT INTO inbox (id, value, updated_ms) VALUES ('bad', 'not json', 1)",
            [],
        )
        .unwrap();
    assert_eq!(ids(&load(&dir, 1).unwrap()), vec!["a".to_string()]);
}

#[test]
fn load_cuts_long_text() {
    let tmp = TempDir::new("inbox-cut");
    let dir = tmp.path("data");
    let mut long = item("a", 1, State::Expired);
    long.title = "t".repeat(TITLE_MAX + 50);
    long.detail = "d".repeat(DETAIL_MAX + 50);
    save(&dir, &Change::Raised(long)).unwrap();
    let loaded = load(&dir, 2).unwrap().items();
    assert_eq!(loaded[0].title.chars().count(), TITLE_MAX);
    assert!(loaded[0].title.ends_with('…'));
    assert_eq!(loaded[0].detail.chars().count(), DETAIL_MAX);
    assert!(loaded[0].detail.ends_with('…'));
}

fn paths(tmp: &TempDir) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let data = tmp.path("data");
    let cluster = tmp.path("cluster");
    (
        data.join(store::DB_NAME),
        cluster.join(crate::cluster::persist::CLUSTER_DB),
        cluster,
    )
}

fn rowids(db: &std::path::Path) -> Vec<(String, String, i64)> {
    let db = rusqlite::Connection::open(db).unwrap();
    let mut query = db
        .prepare("SELECT id, value, updated_ms FROM inbox ORDER BY rowid")
        .unwrap();
    let rows = query
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    rows
}

#[test]
fn inbox_moves_to_cluster_db_once() {
    let tmp = TempDir::new("inbox-move");
    let (node_db, cluster_db, cluster_dir) = paths(&tmp);
    let data = tmp.path("data");
    save(&data, &Change::Raised(item("a", 1, State::Expired))).unwrap();
    save(&data, &Change::Raised(item("b", 2, State::Expired))).unwrap();
    save(&data, &Change::Raised(item("c", 3, State::Expired))).unwrap();
    assert_eq!(migrate_inbox(&node_db, &cluster_db), Ok(3));
    assert!(rowids(&node_db).is_empty());
    let moved = load_from(&Store::Cluster(cluster_dir.clone()), 100).unwrap();
    assert_eq!(ids(&moved), vec!["c", "b", "a"]);
    assert_eq!(migrate_inbox(&node_db, &cluster_db), Ok(0));
    assert_eq!(rowids(&cluster_db).len(), 3);
    assert!(load(&data, 100).unwrap().items().is_empty());
}

#[test]
fn migration_keeps_ids_and_order() {
    let tmp = TempDir::new("inbox-order");
    let (node_db, cluster_db, _) = paths(&tmp);
    let data = tmp.path("data");
    let mut closed = item("m", 30, State::Answered);
    closed.answer = Some(ActionId::Done);
    closed.closed_ms = Some(40);
    save(&data, &Change::Raised(item("z", 10, State::Expired))).unwrap();
    save(&data, &Change::Raised(closed)).unwrap();
    save(&data, &Change::Raised(item("a", 20, State::Expired))).unwrap();
    let before = rowids(&node_db);
    assert_eq!(migrate_inbox(&node_db, &cluster_db), Ok(3));
    let after = rowids(&cluster_db);
    assert_eq!(after, before);
    let order: Vec<&str> = after.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(order, vec!["z", "m", "a"]);
}

#[test]
fn crash_between_copy_and_delete_is_safe() {
    let tmp = TempDir::new("inbox-crash");
    let (node_db, cluster_db, cluster_dir) = paths(&tmp);
    let data = tmp.path("data");
    save(&data, &Change::Raised(item("a", 1, State::Expired))).unwrap();
    save(&data, &Change::Raised(item("b", 2, State::Expired))).unwrap();
    save(&data, &Change::Raised(item("c", 3, State::Expired))).unwrap();
    let target = Store::Cluster(cluster_dir);
    save_to(&target, &Change::Raised(item("a", 1, State::Expired))).unwrap();
    let mut newer = item("b", 2, State::Answered);
    newer.answer = Some(ActionId::Done);
    newer.closed_ms = Some(9);
    save_to(&target, &Change::Updated(newer.clone())).unwrap();
    assert_eq!(migrate_inbox(&node_db, &cluster_db), Ok(1));
    assert!(rowids(&node_db).is_empty());
    let book = load_from(&target, 100).unwrap();
    assert_eq!(ids(&book), vec!["c", "b", "a"]);
    let b = book.items().into_iter().find(|i| i.id == "b").unwrap();
    assert_eq!(b, newer);
    assert_eq!(rowids(&cluster_db).len(), 3);
}

#[test]
fn node_without_cluster_keeps_notchling_db() {
    let tmp = TempDir::new("inbox-node");
    let (node_db, cluster_db, cluster_dir) = paths(&tmp);
    let data = tmp.path("data");
    assert_eq!(migrate_inbox(&node_db, &cluster_db), Ok(0));
    assert!(!cluster_dir.exists());
    save(&data, &Change::Raised(item("a", 1, State::Expired))).unwrap();
    let node = Store::Node(data.clone());
    save_to(&node, &Change::Raised(item("b", 2, State::Expired))).unwrap();
    assert_eq!(ids(&load_from(&node, 100).unwrap()), vec!["b", "a"]);
    assert!(!cluster_dir.exists());
    assert_eq!(rowids(&node_db).len(), 2);
    let empty = load_from(&Store::Cluster(cluster_dir.clone()), 1).unwrap();
    assert!(ids(&empty).is_empty());
    assert!(!cluster_dir.exists());
}

#[test]
fn leaving_a_cluster_moves_the_inbox_back() {
    let tmp = TempDir::new("inbox-back");
    let (node_db, cluster_db, cluster_dir) = paths(&tmp);
    let data = tmp.path("data");
    let target = Store::Cluster(cluster_dir);
    save_to(&target, &Change::Raised(item("x", 1, State::Expired))).unwrap();
    save_to(&target, &Change::Raised(item("y", 2, State::Expired))).unwrap();
    assert_eq!(migrate_inbox_back(&cluster_db, &node_db), Ok(2));
    assert!(rowids(&cluster_db).is_empty());
    assert_eq!(ids(&load(&data, 100).unwrap()), vec!["y", "x"]);
}

fn answered(id: &str, created_ms: i64, closed_ms: i64) -> Item {
    let mut a = item(id, created_ms, State::Answered);
    a.answer = Some(ActionId::Done);
    a.closed_ms = Some(closed_ms);
    a
}

#[test]
fn a_newer_row_in_notchling_db_wins_over_a_stale_cluster_row() {
    let tmp = TempDir::new("inbox-newer-source");
    let (node_db, cluster_db, cluster_dir) = paths(&tmp);
    let data = tmp.path("data");
    let target = Store::Cluster(cluster_dir);
    save_to(&target, &Change::Raised(item("b", 2, State::Open))).unwrap();
    save(&data, &Change::Raised(answered("b", 2, 50))).unwrap();
    assert_eq!(migrate_inbox(&node_db, &cluster_db), Ok(1));
    assert!(rowids(&node_db).is_empty());
    let book = load_from(&target, 100).unwrap();
    assert_eq!(book.items()[0].state, State::Answered);
    assert_eq!(rowids(&cluster_db).len(), 1);
}

#[test]
fn a_newer_row_in_cluster_db_wins_over_a_stale_notchling_row_going_back() {
    let tmp = TempDir::new("inbox-newer-back");
    let (node_db, cluster_db, cluster_dir) = paths(&tmp);
    let data = tmp.path("data");
    let target = Store::Cluster(cluster_dir);
    save(&data, &Change::Raised(item("b", 2, State::Expired))).unwrap();
    save_to(&target, &Change::Raised(answered("b", 2, 50))).unwrap();
    assert_eq!(migrate_inbox_back(&cluster_db, &node_db), Ok(1));
    assert!(rowids(&cluster_db).is_empty());
    assert_eq!(load(&data, 100).unwrap().items()[0].state, State::Answered);
}

#[test]
fn an_older_source_row_never_replaces_a_newer_target_row() {
    let tmp = TempDir::new("inbox-older-source");
    let (node_db, cluster_db, cluster_dir) = paths(&tmp);
    let data = tmp.path("data");
    let target = Store::Cluster(cluster_dir);
    save_to(&target, &Change::Raised(answered("b", 2, 50))).unwrap();
    save(&data, &Change::Raised(item("b", 2, State::Expired))).unwrap();
    assert_eq!(migrate_inbox(&node_db, &cluster_db), Ok(0));
    assert!(rowids(&node_db).is_empty());
    assert_eq!(
        load_from(&target, 100).unwrap().items()[0].state,
        State::Answered
    );
}

#[test]
fn migration_refuses_paths_that_are_not_the_databases() {
    let tmp = TempDir::new("inbox-names");
    let other = tmp.path("data").join("other.db");
    let (node_db, cluster_db, _) = paths(&tmp);
    assert!(migrate_inbox(&node_db, &other).is_err());
    assert!(migrate_inbox(&other, &cluster_db).is_err());
}

#[test]
fn outbox_unsent_kind_and_its_actions_use_their_wire_words() {
    assert_eq!(
        serde_json::to_value(Kind::OutboxUnsent).unwrap(),
        serde_json::json!("outbox_unsent")
    );
    assert_eq!(
        serde_json::to_value([ActionId::Send, ActionId::Discard]).unwrap(),
        serde_json::json!(["send", "discard"])
    );
    let tmp = TempDir::new("inbox-outbox-kind");
    let dir = tmp.path("data");
    let mut a = item("outbox:s:1", 1, State::Open);
    a.kind = Kind::OutboxUnsent;
    a.actions = vec![Action {
        id: ActionId::Send,
        label: "Send".to_string(),
        primary: true,
    }];
    save(&dir, &Change::Raised(a.clone())).unwrap();
    let book = load(&dir, 2).unwrap();
    assert_eq!(book.items().len(), 1);
    assert_eq!(book.items()[0].kind, Kind::OutboxUnsent);
}

#[test]
fn failover_kinds_and_actions_use_their_wire_words_and_survive_a_save() {
    let kinds = [
        (Kind::PrimaryUnreachable, "primary_unreachable"),
        (Kind::Failover, "failover"),
        (Kind::PausedStep, "paused_step"),
        (Kind::NoPrimary, "no_primary"),
        (Kind::PrimaryBack, "primary_back"),
        (Kind::SessionWaits, "session_waits"),
    ];
    let actions = [
        (ActionId::MoveHere, "move_here"),
        (ActionId::MoveAnyway, "move_anyway"),
        (ActionId::TakeBack, "take_back"),
        (ActionId::Keep, "keep"),
        (ActionId::ContinueHere, "continue_here"),
        (ActionId::WaitFor, "wait_for"),
    ];
    for (kind, word) in kinds {
        assert_eq!(serde_json::to_value(kind).unwrap(), serde_json::json!(word));
        assert_eq!(
            serde_json::from_value::<Kind>(serde_json::json!(word)).unwrap(),
            kind
        );
    }
    for (action, word) in actions {
        assert_eq!(
            serde_json::to_value(action).unwrap(),
            serde_json::json!(word)
        );
        assert_eq!(
            serde_json::from_value::<ActionId>(serde_json::json!(word)).unwrap(),
            action
        );
    }
    let tmp = TempDir::new("inbox-failover-kinds");
    let dir = tmp.path("data");
    for (n, (kind, _)) in kinds.into_iter().enumerate() {
        let mut a = item(&format!("k{n}"), n as i64 + 1, State::Open);
        a.kind = kind;
        a.actions = actions
            .iter()
            .map(|(id, word)| Action {
                id: *id,
                label: (*word).to_string(),
                primary: false,
            })
            .collect();
        save(&dir, &Change::Raised(a)).unwrap();
    }
    let book = load(&dir, 100).unwrap();
    let mut found: Vec<Kind> = book.items().into_iter().map(|i| i.kind).collect();
    found.sort_by_key(|k| format!("{k:?}"));
    let mut want: Vec<Kind> = kinds.iter().map(|(k, _)| *k).collect();
    want.sort_by_key(|k| format!("{k:?}"));
    assert_eq!(found, want);
    assert!(book
        .items()
        .iter()
        .all(|i| i.actions.len() == actions.len()));
}

#[test]
fn a_kind_this_build_does_not_know_reads_as_a_notice_and_the_consent_kind_round_trips() {
    assert_eq!(
        serde_json::from_value::<Kind>(serde_json::json!("kind_from_a_newer_build")).unwrap(),
        Kind::Notice
    );
    assert_eq!(
        serde_json::to_value(Kind::Notice).unwrap(),
        serde_json::json!("notice")
    );
    assert_eq!(
        serde_json::to_value(Kind::Consent).unwrap(),
        serde_json::json!("consent")
    );
    assert_eq!(
        serde_json::from_value::<Kind>(serde_json::json!("consent")).unwrap(),
        Kind::Consent
    );
}
