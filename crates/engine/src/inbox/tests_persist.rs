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
