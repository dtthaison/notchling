use super::category::{on, order, Category};
use crate::settings::spec;

fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

#[test]
fn ids_match_the_keys_allowed_list() {
    let ids: Vec<&str> = Category::ALL.iter().map(|c| c.id()).collect();
    assert_eq!(ids, spec("searchCategories").unwrap().allowed);
    assert_eq!(ids, spec("searchCategoryOrder").unwrap().allowed);
    for c in Category::ALL {
        assert_eq!(Category::from_id(c.id()), Some(c));
    }
    assert_eq!(Category::from_id("bogus"), None);
}

#[test]
fn defaults_follow_the_keys() {
    let default_on: Vec<&str> = Category::ALL
        .iter()
        .filter(|c| c.default_on())
        .map(|c| c.id())
        .collect();
    let want = spec("searchCategories").unwrap().default_value().unwrap();
    assert_eq!(serde_json::json!(default_on), want);
}

#[test]
fn order_fills_missing_and_drops_unknown() {
    let got = order(&strings(&["pages", "bogus"]));
    assert_eq!(got[0], Category::Pages);
    let rest: Vec<Category> = Category::ALL
        .into_iter()
        .filter(|c| *c != Category::Pages)
        .collect();
    assert_eq!(&got[1..], rest.as_slice());
    assert_eq!(order(&[]), Category::ALL.to_vec());
    assert_eq!(
        order(&strings(&["pages", "pages", "sessions"]))[..2],
        [Category::Pages, Category::Sessions]
    );
}

#[test]
fn on_keeps_only_known_ids_once() {
    assert_eq!(
        on(&strings(&["vault", "bogus", "vault", "sessions"])),
        [Category::Vault, Category::Sessions]
    );
}

#[test]
fn caps_are_eight_for_sessions_and_four_otherwise() {
    assert_eq!(Category::Sessions.cap(), 8);
    assert_eq!(Category::Pages.cap(), 4);
    assert_eq!(Category::Vault.cap(), 4);
}

#[test]
fn tiers_say_who_answers() {
    assert!(Category::Sessions.in_daemon() && Category::Pages.in_daemon());
    assert!(!Category::Actions.in_daemon() && !Category::Inbox.in_daemon());
    assert!(Category::Vault.app_tier() && !Category::Sessions.app_tier());
}
