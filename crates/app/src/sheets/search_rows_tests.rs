use std::collections::HashMap;

use notchling_engine::search::query::category::Category;
use serde_json::{json, Value};

use super::*;
use crate::acting::Acting;
use crate::sheets::settings::{change as change_row, sections, Daemon, Field};

fn reader(pairs: &[(&str, Value)]) -> impl Fn(&str) -> Option<Value> {
    let store: HashMap<String, Value> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect();
    move |key: &str| store.get(key).cloned()
}

fn defaults() -> Vec<(Category, bool)> {
    categories(&reader(&[]))
}

fn search_section(read: &dyn Fn(&str) -> Option<Value>) -> crate::sheets::settings::Section {
    let daemon = Daemon {
        acting: Acting::Off,
        ..Daemon::detached()
    };
    sections(read, &daemon)
        .into_iter()
        .find(|s| s.title == notchling_locale::settings::section_search())
        .expect("a Search section")
}

fn ids(list: &[String]) -> Vec<&str> {
    list.iter().map(String::as_str).collect()
}

#[test]
fn toggle_writes_only_categories() {
    let rows = defaults();
    assert_eq!(
        ids(&toggle(&rows, Category::Pages)),
        ["sessions", "settings", "inbox", "actions"]
    );
    assert_eq!(
        ids(&toggle(&rows, Category::Vault)),
        ["sessions", "settings", "pages", "inbox", "actions", "vault"]
    );
    let (key, value) = change(&rows, "toggle:pages").unwrap();
    assert_eq!(key, "searchCategories");
    assert_eq!(value, json!(["sessions", "settings", "inbox", "actions"]));
}

#[test]
fn toggle_follows_the_shown_order() {
    let read = reader(&[
        ("searchCategoryOrder", json!(["vault", "inbox"])),
        ("searchCategories", json!(["inbox"])),
    ]);
    let rows = categories(&read);
    assert_eq!(
        ids(&toggle(&rows, Category::Sessions)),
        ["inbox", "sessions"]
    );
}

#[test]
fn move_writes_only_order() {
    let rows = defaults();
    let (key, value) = change(&rows, "down:sessions").unwrap();
    assert_eq!(key, "searchCategoryOrder");
    assert_eq!(
        value,
        json!([
            "settings", "pages", "sessions", "inbox", "actions", "activity", "vault", "machines"
        ])
    );
    let (key, value) = change(&rows, "up:vault").unwrap();
    assert_eq!(key, "searchCategoryOrder");
    assert_eq!(
        value,
        json!([
            "sessions", "settings", "vault", "pages", "inbox", "actions", "activity", "machines"
        ])
    );
}

#[test]
fn a_move_stops_at_the_ends() {
    let rows = defaults();
    let same = [
        "sessions", "settings", "pages", "inbox", "actions", "activity", "vault", "machines",
    ];
    assert_eq!(ids(&move_row(&rows, Category::Sessions, -1)), same);
    assert_eq!(ids(&move_row(&rows, Category::Vault, 1)), same);
    assert_eq!(
        ids(&move_row(&rows, Category::Sessions, 9)),
        ["settings", "pages", "inbox", "actions", "activity", "vault", "sessions", "machines"]
    );
    assert_eq!(ids(&move_row(&rows, Category::Activity, 1)), same);
    assert_eq!(ids(&move_row(&rows, Category::Settings, 1)), same);
}

#[test]
fn unknown_options_change_nothing() {
    let rows = defaults();
    assert_eq!(change(&rows, "toggle:nothing"), None);
    assert_eq!(change(&rows, "flip:pages"), None);
    assert_eq!(change(&rows, "pages"), None);
    assert_eq!(change(&rows, "toggle:activity"), None);
    assert_eq!(change(&rows, "toggle:settings"), None);
    assert_eq!(change(&rows, "up:inbox"), None);
}

#[test]
fn the_rows_follow_the_stored_order_and_checks() {
    let read = reader(&[
        (
            "searchCategoryOrder",
            json!(["inbox", "sessions", "activity"]),
        ),
        ("searchCategories", json!(["sessions", "vault", "machines"])),
    ]);
    assert_eq!(
        categories(&read),
        [
            (Category::Inbox, false),
            (Category::Sessions, true),
            (Category::Activity, false),
            (Category::Settings, false),
            (Category::Pages, false),
            (Category::Actions, false),
            (Category::Vault, true),
            (Category::Machines, true),
        ]
    );
}

#[test]
fn the_defaults_are_all_but_vault() {
    assert_eq!(
        defaults(),
        [
            (Category::Sessions, true),
            (Category::Settings, true),
            (Category::Pages, true),
            (Category::Inbox, true),
            (Category::Actions, true),
            (Category::Activity, false),
            (Category::Vault, false),
            (Category::Machines, false),
        ]
    );
}

#[test]
fn the_search_section_shows_range_and_categories() {
    let section = search_section(&reader(&[]));
    let keys: Vec<&str> = section.rows.iter().map(|r| r.key).collect();
    assert_eq!(
        keys,
        [
            "searchShortcut",
            "semanticSearch",
            "searchIndexRange",
            "searchCategories"
        ]
    );
    let Field::Choice { options, selected } = &section.rows[2].field else {
        panic!("the range is a choice");
    };
    assert_eq!(selected, "all");
    assert_eq!(
        options.iter().map(|(v, _)| *v).collect::<Vec<_>>(),
        ["14d", "30d", "all"]
    );
    let Field::Categories(rows) = &section.rows[3].field else {
        panic!("the categories are a list");
    };
    assert_eq!(rows, &defaults());
    let shown: Vec<Category> = lines(rows).iter().map(|l| l.category).collect();
    assert_eq!(
        shown,
        [Category::Sessions, Category::Pages, Category::Vault]
    );
}

#[test]
fn the_range_shows_the_stored_choice() {
    let section = search_section(&reader(&[("searchIndexRange", json!("14d"))]));
    let Field::Choice { selected, .. } = &section.rows[2].field else {
        panic!("a choice");
    };
    assert_eq!(selected, "14d");
    let (key, value) = change_row(&section.rows[2], "30d").unwrap();
    assert_eq!((key, value), ("searchIndexRange", json!("30d")));
    assert_eq!(change_row(&section.rows[2], "week"), None);
}

#[test]
fn the_categories_row_changes_through_the_same_path() {
    let section = search_section(&reader(&[]));
    let (key, value) = change_row(&section.rows[3], "toggle:pages").unwrap();
    assert_eq!(key, "searchCategories");
    assert_eq!(value, json!(["sessions", "settings", "inbox", "actions"]));
    let (key, _) = change_row(&section.rows[3], "up:pages").unwrap();
    assert_eq!(key, "searchCategoryOrder");
}

#[test]
fn only_the_range_and_the_categories_ask_for_a_refresh() {
    assert!(is_search_key("searchIndexRange"));
    assert!(is_search_key("searchCategories"));
    assert!(!is_search_key("searchCategoryOrder"));
    assert!(!is_search_key("searchShortcut"));
}

#[test]
fn lines_mark_the_first_and_the_last() {
    let lines = lines(&defaults());
    assert_eq!(lines.len(), 3);
    assert!(lines[0].first && !lines[0].last);
    assert!(lines[2].last && !lines[2].first);
    assert!(!lines[1].first && !lines[1].last);
    assert_eq!(lines[0].title, title(Category::Sessions));
    assert_eq!(lines[2].title, "Vault Names");
    assert!(lines[0].on && !lines[2].on);
}

#[test]
fn every_shown_category_has_a_title() {
    let titles: Vec<&str> = SHOWN.iter().map(|c| title(*c)).collect();
    assert_eq!(titles, ["Sessions", "Pages", "Vault Names"]);
}

#[test]
fn settings_inbox_and_actions_stay_hidden_until_something_searches_them() {
    let shown: Vec<&str> = lines(&defaults()).iter().map(|l| l.title).collect();
    assert_eq!(shown, ["Sessions", "Pages", "Vault Names"]);
    for hidden in [Category::Settings, Category::Inbox, Category::Actions] {
        assert!(!SHOWN.contains(&hidden));
    }
}

#[test]
fn saving_the_visible_rows_keeps_the_hidden_values() {
    let read = reader(&[(
        "searchCategories",
        json!(["sessions", "settings", "actions"]),
    )]);
    let rows = categories(&read);
    assert_eq!(
        ids(&toggle(&rows, Category::Pages)),
        ["sessions", "settings", "pages", "actions"]
    );
    assert_eq!(
        ids(&toggle(&rows, Category::Sessions)),
        ["settings", "actions"]
    );
    let (key, value) = change(&rows, "toggle:vault").unwrap();
    assert_eq!(key, "searchCategories");
    assert_eq!(value, json!(["sessions", "settings", "actions", "vault"]));

    let off = reader(&[("searchCategories", json!(["sessions"]))]);
    let rows = categories(&off);
    assert_eq!(ids(&toggle(&rows, Category::Pages)), ["sessions", "pages"]);
    assert_eq!(ids(&toggle(&rows, Category::Vault)), ["sessions", "vault"]);

    let section = search_section(&read);
    let (key, value) = change_row(&section.rows[3], "toggle:pages").unwrap();
    assert_eq!(key, "searchCategories");
    assert_eq!(value, json!(["sessions", "settings", "pages", "actions"]));
}

#[test]
fn moving_a_visible_row_keeps_the_hidden_ones_in_place() {
    let rows = defaults();
    assert_eq!(
        ids(&move_row(&rows, Category::Pages, -1)),
        ["pages", "sessions", "settings", "inbox", "actions", "activity", "vault", "machines"]
    );
    assert_eq!(
        ids(&move_row(&rows, Category::Pages, 1)),
        ["sessions", "settings", "inbox", "actions", "activity", "vault", "pages", "machines"]
    );
}
