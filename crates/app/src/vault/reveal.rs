use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use notchling_engine::vault::entry::{Kind, VaultValue};

pub const AUTO_HIDE_MS: i64 = 30_000;
pub const CLIPBOARD_CLEAR_MS: u64 = 60_000;
pub const MASK: &str = "••••••••";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    Username,
    Secret,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Word {
    Username,
    Password,
    Token,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub part: Part,
    pub word: Word,
    pub copy: bool,
    pub eye: bool,
}

impl Slot {
    pub fn secret(self) -> bool {
        self.part == Part::Secret
    }
}

pub fn slots(kind: Kind) -> Vec<Slot> {
    let secret = |word| Slot {
        part: Part::Secret,
        word,
        copy: true,
        eye: true,
    };
    match kind {
        Kind::Login => vec![
            Slot {
                part: Part::Username,
                word: Word::Username,
                copy: true,
                eye: false,
            },
            secret(Word::Password),
        ],
        Kind::Token => vec![secret(Word::Token)],
    }
}

pub fn part_of(value: &VaultValue, part: Part) -> String {
    match part {
        Part::Username => value.username.clone(),
        Part::Secret => value.secret.clone(),
    }
}

pub fn masked() -> &'static str {
    MASK
}

pub fn display(shown: Option<&str>) -> String {
    match shown {
        Some(value) => value.replace(['\n', '\r'], " "),
        None => masked().to_owned(),
    }
}

pub fn expired(since: i64, now: i64) -> bool {
    now.saturating_sub(since) >= AUTO_HIDE_MS
}

struct Held {
    value: String,
    rev: String,
    since: i64,
}

#[derive(Default)]
pub struct Revealed {
    held: HashMap<(String, Part), Held>,
}

impl Revealed {
    pub fn show(&mut self, name: &str, part: Part, rev: &str, value: String, now: i64) {
        self.held.insert(
            (name.to_owned(), part),
            Held {
                value,
                rev: rev.to_owned(),
                since: now,
            },
        );
    }

    pub fn hide(&mut self, name: &str, part: Part) {
        self.held.remove(&(name.to_owned(), part));
    }

    pub fn hide_entry(&mut self, name: &str) {
        self.held.retain(|(n, _), _| n != name);
    }

    pub fn hide_all(&mut self) {
        self.held.clear();
    }

    pub fn is_shown(&self, name: &str, part: Part) -> bool {
        self.held.contains_key(&(name.to_owned(), part))
    }

    pub fn value(&self, name: &str, part: Part, now: i64) -> Option<&str> {
        self.held
            .get(&(name.to_owned(), part))
            .filter(|h| !expired(h.since, now))
            .map(|h| h.value.as_str())
    }

    pub fn sweep(&mut self, now: i64) -> bool {
        let before = self.held.len();
        self.held.retain(|_, h| !expired(h.since, now));
        self.held.len() != before
    }

    pub fn keep_current(&mut self, current: &[(&str, String)]) -> bool {
        let before = self.held.len();
        self.held.retain(|(n, _), h| {
            current
                .iter()
                .any(|(name, rev)| *name == n.as_str() && *rev == h.rev)
        });
        self.held.len() != before
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormShown {
    since: Option<i64>,
}

impl FormShown {
    pub fn hidden() -> FormShown {
        FormShown { since: None }
    }

    pub fn toggle(self, now: i64) -> FormShown {
        match self.since {
            Some(_) => FormShown::hidden(),
            None => FormShown { since: Some(now) },
        }
    }

    pub fn shows(self, now: i64) -> bool {
        self.since.is_some_and(|s| !expired(s, now))
    }

    pub fn sweep(self, now: i64) -> FormShown {
        if self.shows(now) {
            self
        } else {
            FormShown::hidden()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Written(pub i64);

impl Written {
    pub fn clears(self, change_count: i64) -> bool {
        self.0 == change_count
    }
}

pub struct Epoch(AtomicU64);

impl Epoch {
    pub const fn new() -> Epoch {
        Epoch(AtomicU64::new(0))
    }

    pub fn bump(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

impl Default for Epoch {
    fn default() -> Epoch {
        Epoch::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen(u64);

impl Seen {
    pub fn of(epoch: &Epoch) -> Seen {
        Seen(epoch.get())
    }

    pub fn moved(&mut self, epoch: &Epoch) -> bool {
        let now = epoch.get();
        let moved = now != self.0;
        self.0 = now;
        moved
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hide {
    pub reads: bool,
    pub form: bool,
}

pub fn hides(can_read: bool, vault_on: bool) -> Hide {
    Hide {
        reads: !can_read,
        form: !vault_on,
    }
}

pub fn clears_clipboard(part: Part) -> bool {
    part == Part::Secret
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE: &str = "fake-value-1";

    #[test]
    fn the_mask_is_fixed_and_never_shows_a_length_or_a_character() {
        assert_eq!(display(None), MASK);
        assert_eq!(display(None), masked());
        assert_eq!(display(Some(FAKE)), FAKE);
        assert!(!MASK.contains(FAKE));
        assert_eq!(display(Some("fake\nline")), "fake line");
        assert_eq!(MASK.chars().count(), 8);
    }

    #[test]
    fn a_login_has_a_plain_username_with_copy_and_a_password_with_copy_and_eye() {
        let login = slots(Kind::Login);
        assert_eq!(login.len(), 2);
        assert_eq!(login[0].word, Word::Username);
        assert!(login[0].copy && !login[0].eye && !login[0].secret());
        assert_eq!(login[1].word, Word::Password);
        assert!(login[1].copy && login[1].eye && login[1].secret());
    }

    #[test]
    fn a_token_has_one_secret_with_copy_and_eye() {
        let token = slots(Kind::Token);
        assert_eq!(token.len(), 1);
        assert_eq!(token[0].word, Word::Token);
        assert!(token[0].copy && token[0].eye && token[0].secret());
    }

    #[test]
    fn every_secret_slot_has_an_eye_and_no_plain_slot_has_one() {
        for kind in [Kind::Login, Kind::Token] {
            for slot in slots(kind) {
                assert_eq!(slot.eye, slot.secret());
                assert!(slot.copy);
            }
        }
    }

    #[test]
    fn a_value_hides_after_thirty_seconds_and_not_before() {
        assert!(!expired(1_000, 1_000 + AUTO_HIDE_MS - 1));
        assert!(expired(1_000, 1_000 + AUTO_HIDE_MS));
        let mut shown = Revealed::default();
        shown.show("a", Part::Secret, "r1", FAKE.into(), 1_000);
        assert_eq!(shown.value("a", Part::Secret, 1_000), Some(FAKE));
        assert_eq!(
            shown.value("a", Part::Secret, 1_000 + AUTO_HIDE_MS - 1),
            Some(FAKE)
        );
        assert_eq!(shown.value("a", Part::Secret, 1_000 + AUTO_HIDE_MS), None);
        assert!(!shown.sweep(1_000 + AUTO_HIDE_MS - 1));
        assert!(shown.sweep(1_000 + AUTO_HIDE_MS));
        assert!(shown.is_empty());
    }

    #[test]
    fn hiding_one_field_one_entry_or_everything_drops_only_those() {
        let mut shown = Revealed::default();
        shown.show("a", Part::Secret, "r1", "one".into(), 0);
        shown.show("a", Part::Username, "r1", "two".into(), 0);
        shown.show("b", Part::Secret, "r1", "three".into(), 0);
        shown.hide("a", Part::Username);
        assert!(shown.is_shown("a", Part::Secret) && !shown.is_shown("a", Part::Username));
        shown.hide_entry("a");
        assert!(!shown.is_shown("a", Part::Secret) && shown.is_shown("b", Part::Secret));
        shown.hide_all();
        assert!(shown.is_empty());
    }

    fn current(list: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
        list.iter().map(|(n, r)| (*n, (*r).to_owned())).collect()
    }

    #[test]
    fn an_entry_that_left_the_list_loses_its_shown_value() {
        let mut shown = Revealed::default();
        shown.show("a", Part::Secret, "r1", "one".into(), 0);
        shown.show("b", Part::Secret, "r1", "two".into(), 0);
        assert!(shown.keep_current(&current(&[("b", "r1")])));
        assert!(!shown.is_shown("a", Part::Secret) && shown.is_shown("b", Part::Secret));
        assert!(!shown.keep_current(&current(&[("b", "r1")])));
    }

    #[test]
    fn an_entry_saved_again_or_replaced_under_the_same_name_loses_its_shown_value() {
        let mut shown = Revealed::default();
        shown.show("a", Part::Secret, "t1#1", "old".into(), 0);
        shown.show("b", Part::Secret, "t1#1", "two".into(), 0);
        assert!(shown.keep_current(&current(&[("a", "t1#2"), ("b", "t1#1")])));
        assert!(!shown.is_shown("a", Part::Secret));
        assert!(shown.is_shown("b", Part::Secret));
        shown.show("a", Part::Secret, "t1#2", "new".into(), 0);
        assert!(shown.keep_current(&current(&[("a", "t9"), ("b", "t1#1")])));
        assert!(!shown.is_shown("a", Part::Secret));
    }

    #[test]
    fn a_lock_or_a_sleep_is_seen_once_by_each_view() {
        let locked = Epoch::new();
        let mut first = Seen::of(&locked);
        let mut second = Seen::of(&locked);
        assert!(!first.moved(&locked));
        locked.bump();
        assert!(first.moved(&locked));
        assert!(!first.moved(&locked));
        assert!(second.moved(&locked));
        assert!(!second.moved(&locked));
        locked.bump();
        locked.bump();
        assert!(first.moved(&locked));
    }

    #[test]
    fn the_remote_tab_cannot_read_but_keeps_its_form_eye_and_a_vault_that_is_off_hides_it() {
        assert_eq!(
            hides(true, true),
            Hide {
                reads: false,
                form: false
            }
        );
        assert_eq!(
            hides(false, true),
            Hide {
                reads: true,
                form: false
            }
        );
        assert_eq!(
            hides(false, false),
            Hide {
                reads: true,
                form: true
            }
        );
        assert_eq!(
            hides(true, false),
            Hide {
                reads: false,
                form: true
            }
        );
    }

    #[test]
    fn the_form_eye_toggles_and_hides_on_its_own_after_thirty_seconds() {
        let hidden = FormShown::hidden();
        assert!(!hidden.shows(0));
        let on = hidden.toggle(500);
        assert!(on.shows(500) && on.shows(500 + AUTO_HIDE_MS - 1));
        assert!(!on.shows(500 + AUTO_HIDE_MS));
        assert_eq!(on.sweep(500 + AUTO_HIDE_MS), FormShown::hidden());
        assert_eq!(on.sweep(600), on);
        assert_eq!(on.toggle(700), FormShown::hidden());
    }

    #[test]
    fn the_clipboard_is_cleared_only_while_it_is_still_the_copy_we_made() {
        let ours = Written(41);
        assert!(ours.clears(41));
        assert!(!ours.clears(42));
        assert!(!ours.clears(40));
        assert!(!ours.clears(0));
    }

    #[test]
    fn only_a_secret_is_cleared_from_the_clipboard() {
        assert!(clears_clipboard(Part::Secret));
        assert!(!clears_clipboard(Part::Username));
        assert_eq!(CLIPBOARD_CLEAR_MS, 60_000);
    }

    #[test]
    fn a_part_is_taken_from_the_value_by_its_name() {
        let value = VaultValue {
            username: "fake-user".into(),
            secret: FAKE.into(),
        };
        assert_eq!(part_of(&value, Part::Username), "fake-user");
        assert_eq!(part_of(&value, Part::Secret), FAKE);
    }
}
