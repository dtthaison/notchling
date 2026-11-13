use super::wire::{Page, Results, SessionRow};
use crate::notch::answer::{type_key, Typed};

pub mod layout {
    pub const WIDTH: f32 = 680.0;
    pub const FIELD: f32 = 56.0;
    pub const PAGE_ROW: f32 = 48.0;
    pub const SESSION_ROW: f32 = 54.0;
    pub const HEADER: f32 = 26.0;
    pub const LIST_PADDING: f32 = 6.0;
    pub const EMPTY: f32 = 52.0;
    pub const FOOTER: f32 = 30.0;
    pub const MAX_LIST: f32 = 8.0 * 48.0;
}

pub const NOTE_MS: i64 = 1600;
pub const EMPTY_LIMIT: usize = 6;
pub const LIMIT: usize = 30;
pub const PAGE_CAP: usize = 20;
pub const SECRET_CAP: usize = 5;
pub const INDEXING: &str = "Indexing sessions…";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Pages,
    Vault,
    Sessions,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Pages => "Pages",
            Section::Vault => "Vault",
            Section::Sessions => "Sessions",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Page(Page),
    Secret(notchling_engine::vault::VaultEntry),
    Session(SessionRow),
}

impl Item {
    pub fn id(&self) -> String {
        match self {
            Item::Page(p) => format!("page:{}", p.key()),
            Item::Secret(e) => format!("secret:{}", e.name),
            Item::Session(s) => format!("session:{}", s.id),
        }
    }

    pub fn section(&self) -> Section {
        match self {
            Item::Page(_) => Section::Pages,
            Item::Secret(_) => Section::Vault,
            Item::Session(_) => Section::Sessions,
        }
    }

    pub fn row_height(&self) -> f32 {
        match self {
            Item::Page(_) | Item::Secret(_) => layout::PAGE_ROW,
            Item::Session(_) => layout::SESSION_ROW,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Press {
    pub key: String,
    pub key_char: Option<String>,
    pub platform: bool,
    pub control: bool,
    pub alt: bool,
}

impl Press {
    pub fn plain(key: &str, key_char: Option<&str>) -> Press {
        Press {
            key: key.into(),
            key_char: key_char.map(str::to_owned),
            platform: false,
            control: false,
            alt: false,
        }
    }

    pub fn command(key: &str) -> Press {
        Press {
            platform: true,
            ..Press::plain(key, None)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Moved,
    Open(Item),
    Hide { returning: bool },
    Cleared,
    Typed,
    Copy { text: String, note: &'static str },
    Ignored,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchModel {
    pub query: String,
    items: Vec<Item>,
    pub selected: usize,
    pub searching: bool,
    pub indexing: bool,
    pub error: Option<String>,
    note: Option<(String, i64)>,
}

impl Default for SearchModel {
    fn default() -> Self {
        SearchModel::new()
    }
}

impl SearchModel {
    pub fn new() -> SearchModel {
        SearchModel {
            query: String::new(),
            items: Vec::new(),
            selected: 0,
            searching: false,
            indexing: false,
            error: None,
            note: None,
        }
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn current(&self) -> Option<&Item> {
        self.items.get(self.selected)
    }

    pub fn limit(&self) -> usize {
        if self.query.trim().is_empty() {
            EMPTY_LIMIT
        } else {
            LIMIT
        }
    }

    pub fn apply(&mut self, results: Results, keeping: Option<&str>) {
        let cap = if self.query.trim().is_empty() {
            EMPTY_LIMIT
        } else {
            PAGE_CAP
        };
        self.items = results
            .pages
            .into_iter()
            .take(cap)
            .map(Item::Page)
            .chain(
                results
                    .secrets
                    .into_iter()
                    .filter(|_| !self.query.trim().is_empty())
                    .take(SECRET_CAP)
                    .map(Item::Secret),
            )
            .chain(results.sessions.into_iter().map(Item::Session))
            .collect();
        self.indexing = results.indexing;
        self.selected = keeping
            .and_then(|id| self.items.iter().position(|item| item.id() == id))
            .unwrap_or(0);
    }

    pub fn starts_section(&self, index: usize) -> bool {
        match (
            index.checked_sub(1).and_then(|i| self.items.get(i)),
            self.items.get(index),
        ) {
            (_, None) => false,
            (None, Some(_)) => true,
            (Some(before), Some(item)) => before.section() != item.section(),
        }
    }

    pub fn list_height(&self) -> f32 {
        let full: f32 = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                item.row_height()
                    + if self.starts_section(i) {
                        layout::HEADER
                    } else {
                        0.0
                    }
            })
            .sum();
        full.min(layout::MAX_LIST)
    }

    pub fn height(&self) -> f32 {
        let chrome = layout::FIELD + 1.0 + layout::FOOTER;
        if self.items.is_empty() {
            chrome + layout::EMPTY + 1.0
        } else {
            chrome + self.list_height() + 2.0 * layout::LIST_PADDING + 1.0
        }
    }

    pub fn move_by(&mut self, by: i32) {
        if self.items.is_empty() {
            return;
        }
        let last = self.items.len() - 1;
        let at = i64::from(by) + self.selected as i64;
        self.selected = at.clamp(0, last as i64) as usize;
    }

    pub fn empty_text(&self) -> &'static str {
        if self.error.is_some() {
            "Search Unavailable"
        } else if self.query.is_empty() {
            if self.indexing || self.searching {
                INDEXING
            } else {
                "Nothing Saved Yet"
            }
        } else if self.searching || self.indexing {
            "Searching…"
        } else {
            "No Matches"
        }
    }

    pub fn flash(&mut self, text: &str, now_ms: i64) {
        self.note = Some((text.to_owned(), now_ms + NOTE_MS));
    }

    pub fn note(&self, now_ms: i64) -> Option<&str> {
        match &self.note {
            Some((text, until)) if now_ms < *until => Some(text),
            _ => None,
        }
    }

    pub fn footer_text(&self, now_ms: i64) -> String {
        if let Some(note) = self.note(now_ms) {
            return note.to_owned();
        }
        if let Some(error) = &self.error {
            return error.clone();
        }
        if self.indexing {
            return INDEXING.to_owned();
        }
        "Pages, Vault and Sessions".to_owned()
    }

    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        let mut hints = Vec::new();
        match self.current() {
            Some(Item::Page(page)) => {
                hints.push(("↩", "Open"));
                if !page.link.is_empty() {
                    hints.push(("⌘C", "Copy Link"));
                }
            }
            Some(Item::Secret(_)) => {
                hints.push(("↩", "Edit"));
                hints.push(("⌘C", "Copy Name"));
            }
            Some(Item::Session(_)) => {
                hints.push(("↩", "Open in Chat"));
                hints.push(("⌘C", "Copy Session ID"));
            }
            None => {}
        }
        hints.push(("esc", "Close"));
        hints
    }

    pub fn press(&mut self, press: &Press) -> Outcome {
        match press.key.as_str() {
            "down" => {
                self.move_by(1);
                return Outcome::Moved;
            }
            "up" => {
                self.move_by(-1);
                return Outcome::Moved;
            }
            "enter" => {
                return match self.current() {
                    Some(item) => Outcome::Open(item.clone()),
                    None => Outcome::Ignored,
                };
            }
            "escape" => {
                return if self.query.is_empty() {
                    Outcome::Hide { returning: true }
                } else {
                    self.query.clear();
                    Outcome::Cleared
                };
            }
            _ => {}
        }
        if press.platform {
            return match press.key.as_str() {
                "c" => match self.current() {
                    Some(Item::Page(page)) if !page.link.is_empty() => Outcome::Copy {
                        text: page.link.clone(),
                        note: "Link Copied",
                    },
                    Some(Item::Secret(entry)) => Outcome::Copy {
                        text: entry.name.clone(),
                        note: "Name Copied",
                    },
                    Some(Item::Session(session)) => Outcome::Copy {
                        text: session.session_id.clone(),
                        note: "Session ID Copied",
                    },
                    _ => Outcome::Ignored,
                },
                "w" => Outcome::Hide { returning: true },
                _ => Outcome::Ignored,
            };
        }
        match type_key(
            &mut self.query,
            &press.key,
            press.key_char.as_deref(),
            press.control,
        ) {
            Typed::Changed => Outcome::Typed,
            Typed::Submit | Typed::Ignored => Outcome::Ignored,
        }
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
