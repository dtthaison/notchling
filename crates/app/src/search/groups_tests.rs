use super::*;
use crate::search::wire::{Page, SessionRow};
use crate::testutil::Scratch;
use notchling_engine::search::SessionTool;
use notchling_engine::vault::{Kind, VaultEntry};
use serde_json::json;

fn results(pages: usize, secrets: usize, sessions: usize) -> Results {
    Results {
        pages: (0..pages)
            .map(|i| Page {
                folder: format!("p-{i}"),
                ..Page::default()
            })
            .collect(),
        secrets: (0..secrets)
            .map(|i| VaultEntry::new(&format!("secret-{i}"), Kind::Token))
            .collect(),
        sessions: (0..sessions)
            .map(|i| SessionRow {
                id: format!("claude:s{i}"),
                tool: SessionTool::Claude,
                session_id: format!("s{i}"),
                title: format!("Session {i}"),
                folder: String::new(),
                snippet: None,
                highlights: Vec::new(),
                updated_secs: 0.0,
                parent: None,
                related: false,
            })
            .collect(),
        indexing: false,
    }
}

fn sections(items: &[Item]) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for item in items {
        if out.last() != Some(&item.section()) {
            out.push(item.section());
        }
    }
    out
}

fn prefs(order: &[Category], on: &[Category]) -> Prefs {
    Prefs {
        order: order.to_vec(),
        on: on.to_vec(),
    }
}

#[test]
fn groups_follow_the_users_order() {
    let all = [Category::Pages, Category::Vault, Category::Sessions];
    let items = prefs(
        &[Category::Sessions, Category::Pages, Category::Vault],
        &all,
    )
    .arrange(results(1, 1, 1), false);
    assert_eq!(
        sections(&items),
        [Section::Sessions, Section::Pages, Section::Vault]
    );
    let items = prefs(&all, &all).arrange(results(1, 1, 1), false);
    assert_eq!(
        sections(&items),
        [Section::Pages, Section::Vault, Section::Sessions]
    );
}

#[test]
fn an_unchecked_category_shows_nothing_even_when_it_was_answered() {
    let items = prefs(&Category::ALL, &[Category::Sessions]).arrange(results(2, 2, 2), false);
    assert_eq!(sections(&items), [Section::Sessions]);
    assert_eq!(items.len(), 2);
}

#[test]
fn the_panels_own_caps_stay() {
    let all = prefs(&Category::ALL, &Category::ALL);
    let items = all.arrange(results(30, 9, 3), false);
    let count = |s: Section| items.iter().filter(|i| i.section() == s).count();
    assert_eq!(count(Section::Pages), 20);
    assert_eq!(count(Section::Vault), 5);
    assert_eq!(count(Section::Sessions), 3);
    let items = all.arrange(results(30, 9, 3), true);
    assert_eq!(
        items
            .iter()
            .filter(|i| i.section() == Section::Pages)
            .count(),
        6
    );
    assert_eq!(
        items
            .iter()
            .filter(|i| i.section() == Section::Vault)
            .count(),
        0
    );
}

#[test]
fn the_daemon_is_asked_only_for_the_corpora_that_are_on() {
    assert_eq!(Prefs::default().corpora(), ["sessions", "pages"]);
    assert_eq!(
        prefs(&Category::ALL, &[Category::Pages, Category::Vault]).corpora(),
        ["pages"]
    );
    assert!(prefs(&Category::ALL, &[]).corpora().is_empty());
}

#[test]
fn the_defaults_are_the_keys_defaults_and_the_store_overrides_them() {
    let s = Scratch::new("search-prefs");
    let default = Prefs::read(s.path());
    assert_eq!(default, Prefs::default());
    assert_eq!(default.order[0], Category::Sessions);
    assert!(default.shows(Section::Pages) && !default.shows(Section::Vault));
    let mut store = Settings::open(&s.path().join("settings.json")).expect("open");
    store
        .set_unchecked("searchCategoryOrder", json!(["pages", "bogus"]))
        .expect("set");
    store
        .set("searchCategories", json!(["pages", "vault"]))
        .expect("set");
    let read = Prefs::read(s.path());
    assert_eq!(read.order[0], Category::Pages);
    assert_eq!(read.order.len(), Category::ALL.len());
    assert_eq!(read.on, [Category::Pages, Category::Vault]);
}
