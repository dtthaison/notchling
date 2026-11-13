use super::*;

fn action(id: ActionId) -> Action {
    Action {
        id,
        label: format!("{id:?}"),
        primary: false,
    }
}

fn item(id: &str, kind: Kind, created_ms: i64, actions: &[ActionId]) -> Item {
    Item {
        id: id.to_string(),
        kind,
        title: format!("title {id}"),
        detail: String::new(),
        source: Source::default(),
        created_ms,
        expires_ms: None,
        actions: actions.iter().copied().map(action).collect(),
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: None,
        closed_ms: None,
    }
}

fn open_count(book: &Book) -> usize {
    book.items()
        .iter()
        .filter(|item| item.state == State::Open)
        .count()
}

#[test]
fn answer_sets_state_and_first_wins() {
    let mut book = Book::new();
    book.raise(item(
        "a",
        Kind::SecretApproval,
        1,
        &[ActionId::Allow, ActionId::Deny],
    ));
    let change = book.answer("a", ActionId::Allow, "app", 5).unwrap();
    let Change::Updated(updated) = change else {
        panic!("expected Updated");
    };
    assert_eq!(updated.state, State::Answered);
    assert_eq!(updated.answer, Some(ActionId::Allow));
    assert_eq!(updated.answered_by.as_deref(), Some("app"));
    assert_eq!(
        book.answer("a", ActionId::Deny, "phone", 6),
        Err(AnswerError::NotOpen(State::Answered))
    );
    assert_eq!(
        book.answer("missing", ActionId::Deny, "phone", 6),
        Err(AnswerError::Unknown)
    );
}

#[test]
fn answer_rejects_action_not_offered() {
    let mut book = Book::new();
    book.raise(item("a", Kind::Notice, 1, &[ActionId::Done]));
    assert_eq!(
        book.answer("a", ActionId::Allow, "app", 2),
        Err(AnswerError::NotAnAction)
    );
    assert_eq!(book.items()[0].state, State::Open);
}

#[test]
fn expire_turns_open_past_deadline_to_expired() {
    let mut book = Book::new();
    let mut expiring = item("a", Kind::SecretApproval, 1, &[ActionId::Allow]);
    expiring.expires_ms = Some(1000);
    book.raise(expiring);
    assert!(book.expire(999).is_empty());
    let changes = book.expire(1000);
    assert_eq!(changes.len(), 1);
    let Change::Updated(updated) = &changes[0] else {
        panic!("expected Updated");
    };
    assert_eq!(updated.state, State::Expired);
    assert!(book.expire(2000).is_empty());
}

#[test]
fn caps_open_items_at_200_dropping_oldest_notices() {
    let mut book = Book::new();
    for n in 0..MAX_OPEN {
        book.raise(item(&format!("n{n}"), Kind::Notice, n as i64, &[]));
    }
    let changes = book.raise(item(
        "approval",
        Kind::SecretApproval,
        500,
        &[ActionId::Allow],
    ));
    assert_eq!(open_count(&book), 200);
    let ids: Vec<String> = book.items().into_iter().map(|item| item.id).collect();
    assert!(!ids.contains(&"n0".to_string()));
    assert!(ids.contains(&"n1".to_string()));
    assert!(ids.contains(&"approval".to_string()));
    assert_eq!(changes.len(), 2);
    assert!(matches!(&changes[0], Change::Raised(item) if item.id == "approval"));
    assert_eq!(changes[1], Change::Removed("n0".to_string()));
}

#[test]
fn cap_withdraws_oldest_secret_when_no_notice_is_open() {
    let mut book = Book::new();
    for n in 0..MAX_OPEN {
        book.raise(item(
            &format!("a{n}"),
            Kind::SecretApproval,
            n as i64,
            &[ActionId::Allow],
        ));
    }
    let changes = book.raise(item("late", Kind::SecretRequest, 500, &[ActionId::Allow]));
    assert_eq!(open_count(&book), 200);
    assert!(
        matches!(&changes[1], Change::Updated(item) if item.id == "a0" && item.state == State::Withdrawn)
    );
}

#[test]
fn cap_counts_only_open_items() {
    let mut book = Book::new();
    for n in 0..MAX_OPEN {
        book.raise(item(
            &format!("n{n}"),
            Kind::Notice,
            n as i64,
            &[ActionId::Done],
        ));
    }
    book.answer("n0", ActionId::Done, "app", 1000).unwrap();
    let changes = book.raise(item("extra", Kind::Notice, 500, &[]));
    assert_eq!(changes.len(), 1);
    assert_eq!(book.items().len(), 201);
    assert_eq!(open_count(&book), 200);
}

#[test]
fn raise_of_existing_id_replaces_and_returns_updated() {
    let mut book = Book::new();
    book.raise(item("a", Kind::Notice, 1, &[]));
    let mut again = item("a", Kind::Notice, 2, &[]);
    again.title = "new".to_string();
    let changes = book.raise(again);
    assert!(matches!(&changes[0], Change::Updated(item) if item.title == "new"));
    assert_eq!(book.items().len(), 1);
}

#[test]
fn prunes_done_after_seven_days() {
    let mut book = Book::new();
    book.raise(item("done", Kind::Notice, 0, &[ActionId::Done]));
    book.raise(item("open", Kind::Notice, 0, &[ActionId::Done]));
    book.raise(item("fresh", Kind::Notice, 1000, &[ActionId::Done]));
    book.answer("done", ActionId::Done, "app", 0).unwrap();
    book.answer("fresh", ActionId::Done, "app", 1000).unwrap();
    book.prune(DONE_KEEP_MS);
    assert_eq!(book.items().len(), 3);
    book.prune(DONE_KEEP_MS + 1);
    let ids: Vec<String> = book.items().into_iter().map(|item| item.id).collect();
    assert!(!ids.contains(&"done".to_string()));
    assert!(ids.contains(&"open".to_string()));
    assert!(ids.contains(&"fresh".to_string()));
}

#[test]
fn cuts_long_text() {
    let mut book = Book::new();
    let mut long = item("a", Kind::Notice, 1, &[]);
    long.title = "x".repeat(500);
    long.detail = "é".repeat(1000);
    book.raise(long);
    let stored = &book.items()[0];
    assert_eq!(stored.title.chars().count(), TITLE_MAX);
    assert!(stored.title.ends_with('…'));
    assert_eq!(stored.detail.chars().count(), DETAIL_MAX);
    assert!(stored.detail.ends_with('…'));

    let mut exact = item("b", Kind::Notice, 2, &[]);
    exact.title = "y".repeat(TITLE_MAX);
    book.raise(exact);
    assert_eq!(book.items()[0].title, "y".repeat(TITLE_MAX));
}

#[test]
fn unread_counts_open_unseen_only() {
    let mut book = Book::new();
    book.raise(item("a", Kind::Notice, 1, &[ActionId::Done]));
    book.raise(item("b", Kind::Notice, 2, &[ActionId::Done]));
    book.raise(item("c", Kind::Notice, 3, &[ActionId::Done]));
    assert_eq!(book.unread(), 3);
    let changes = book.mark_seen(&["a".to_string(), "zzz".to_string()]);
    assert_eq!(changes.len(), 1);
    assert!(book.mark_seen(&["a".to_string()]).is_empty());
    assert_eq!(book.unread(), 2);
    book.answer("b", ActionId::Done, "app", 9).unwrap();
    assert_eq!(book.unread(), 1);
}

#[test]
fn items_are_newest_first_with_ties_newest_inserted_first() {
    let mut book = Book::new();
    book.raise(item("old", Kind::Notice, 1, &[]));
    book.raise(item("tie1", Kind::Notice, 5, &[]));
    book.raise(item("tie2", Kind::Notice, 5, &[]));
    let ids: Vec<String> = book.items().into_iter().map(|item| item.id).collect();
    assert_eq!(ids, ["tie2", "tie1", "old"]);
}

#[test]
fn resolve_changes_open_items_only() {
    let mut book = Book::new();
    book.raise(item("a", Kind::Notice, 1, &[ActionId::Done]));
    let change = book.resolve("a", State::Withdrawn, None, None, 77).unwrap();
    assert!(
        matches!(&change, Change::Updated(item) if item.state == State::Withdrawn && item.closed_ms == Some(77))
    );
    assert!(book.resolve("a", State::Expired, None, None, 78).is_none());
    assert!(book
        .resolve("zzz", State::Expired, None, None, 78)
        .is_none());
}

const DAY: i64 = 86_400_000;

#[test]
fn prune_counts_from_close_time() {
    let mut book = Book::new();
    book.raise(item("a", Kind::Notice, 0, &[ActionId::Done]));
    let Change::Updated(closed) = book.answer("a", ActionId::Done, "app", 8 * DAY).unwrap() else {
        panic!("expected Updated");
    };
    assert_eq!(closed.closed_ms, Some(8 * DAY));
    book.prune(8 * DAY);
    assert_eq!(book.items().len(), 1);
    book.prune(15 * DAY + 1);
    assert!(book.items().is_empty());
}

#[test]
fn expire_sets_close_time() {
    let mut book = Book::new();
    let mut expiring = item("a", Kind::SecretApproval, 0, &[ActionId::Allow]);
    expiring.expires_ms = Some(10);
    book.raise(expiring);
    let changes = book.expire(50);
    assert!(matches!(&changes[0], Change::Updated(item) if item.closed_ms == Some(50)));
}

#[test]
fn cap_never_evicts_the_item_being_raised() {
    let mut book = Book::new();
    for n in 0..MAX_OPEN {
        book.raise(item(&format!("n{n}"), Kind::Notice, n as i64, &[]));
    }
    let changes = book.raise(item(
        "approval",
        Kind::SecretApproval,
        -5,
        &[ActionId::Allow],
    ));
    assert_eq!(open_count(&book), 200);
    assert!(book.items().iter().any(|item| item.id == "approval"));
    assert_eq!(changes[1], Change::Removed("n0".to_string()));
}

#[test]
fn cap_withdraws_the_oldest_other_approval() {
    let mut book = Book::new();
    for n in 0..MAX_OPEN {
        book.raise(item(
            &format!("a{n}"),
            Kind::SecretApproval,
            n as i64 + 10,
            &[ActionId::Allow],
        ));
    }
    let changes = book.raise(item("late", Kind::SecretRequest, 500, &[ActionId::Allow]));
    assert_eq!(open_count(&book), 200);
    assert_eq!(changes.len(), 2);
    let Change::Updated(withdrawn) = &changes[1] else {
        panic!("expected Updated");
    };
    assert_eq!(withdrawn.id, "a0");
    assert_eq!(withdrawn.state, State::Withdrawn);
    assert_eq!(withdrawn.closed_ms, Some(500));
    assert!(book.items().iter().any(|item| item.id == "a0"));
    assert!(book.items().iter().any(|item| item.id == "late"));
}

#[test]
fn raise_of_a_closed_id_changes_nothing() {
    let mut book = Book::new();
    book.raise(item("a", Kind::Notice, 1, &[ActionId::Done]));
    book.answer("a", ActionId::Done, "app", 5).unwrap();
    let before = book.items();
    let mut again = item("a", Kind::Notice, 9, &[ActionId::Done]);
    again.title = "new".to_string();
    assert!(book.raise(again).is_empty());
    assert_eq!(book.items(), before);
}

#[test]
fn prune_keeps_only_the_newest_closed_items() {
    let mut book = Book::new();
    let extra = 5;
    for n in 0..(MAX_DONE + extra) {
        let id = format!("d{n}");
        book.raise(item(&id, Kind::Notice, 0, &[ActionId::Done]));
        let at = i64::try_from(n).unwrap() + 1;
        book.answer(&id, ActionId::Done, "app", at).unwrap();
    }
    book.raise(item("open", Kind::Notice, 0, &[ActionId::Done]));
    book.prune(MAX_DONE as i64 + 100);
    let items = book.items();
    assert_eq!(items.len(), MAX_DONE + 1);
    let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
    assert!(ids.contains(&"open"));
    for n in 0..extra {
        assert!(!ids.contains(&format!("d{n}").as_str()), "d{n}");
    }
    assert!(ids.contains(&format!("d{}", MAX_DONE + extra - 1).as_str()));
    assert!(ids.contains(&format!("d{extra}").as_str()));
}
