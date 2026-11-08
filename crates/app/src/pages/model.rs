//! What the Artifacts window shows, pure (the Swift `PagesModel`): every saved page, the ones the
//! search leaves, the selected one, and what each key does in the list. The GPUI view (`view`,
//! macOS) only lays it out and asks the daemon for the list.

use notchling_engine::search::{find_pages, PageEntry};

use super::dates;
use crate::notch::answer::{type_key, Typed};
use crate::search::model::Press;
use crate::search::wire::{Page, PageList};

/// What a key press asks the view to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The selection moved.
    Moved,
    /// Return: open the selected page in its own window.
    Open(Box<Page>),
    /// The search changed.
    Typed,
    /// Esc with words typed: the search was cleared.
    Cleared,
    Ignored,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PagesModel {
    pages: Vec<Page>,
    /// Those `pages` as the engine's entries, for `find_pages`; kept in step with `pages`.
    entries: Vec<PageEntry>,
    /// The daemon's list stopped short of every page (a reply is one line).
    pub truncated: bool,
    pub search: String,
    /// The selected page's key ([`Page::key`]).
    pub selection: Option<String>,
}

impl PagesModel {
    pub fn new() -> PagesModel {
        PagesModel::default()
    }

    /// Takes the library's list. The selection stays on its page when it is still listed, else
    /// goes to the first page shown.
    pub fn set_pages(&mut self, pages: Vec<Page>) {
        self.set_list(PageList {
            pages,
            truncated: false,
        });
    }

    /// [`PagesModel::set_pages`] with the daemon's note of a list cut short.
    pub fn set_list(&mut self, list: PageList) {
        self.entries = list.pages.iter().map(Page::entry).collect();
        self.pages = list.pages;
        self.truncated = list.truncated;
        let kept = self
            .selection
            .as_deref()
            .is_some_and(|key| self.pages.iter().any(|p| p.key() == key));
        if !kept {
            self.selection = self.shown().first().map(|p| p.key().to_owned());
        }
    }

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// The pages the search leaves: every word in the title, project, account or link, or the
    /// page a pasted link or id names; every page without words.
    pub fn shown(&self) -> Vec<&Page> {
        let found = find_pages(&self.search, &self.entries);
        found
            .into_iter()
            .filter_map(|entry| {
                self.entries
                    .iter()
                    .position(|e| std::ptr::eq(e, entry))
                    .and_then(|i| self.pages.get(i))
            })
            .collect()
    }

    /// The selected page, listed or not.
    pub fn selected(&self) -> Option<&Page> {
        let key = self.selection.as_deref()?;
        self.pages.iter().find(|p| p.key() == key)
    }

    /// Selects a page by key; a page the search hides clears the search first, so it shows.
    pub fn select(&mut self, key: &str) {
        if !self.shown().iter().any(|p| p.key() == key) {
            self.search.clear();
        }
        self.selection = Some(key.to_owned());
    }

    /// "1 artifact", "12 artifacts", and a word when the list was cut short.
    pub fn count_text(&self) -> String {
        let n = self.shown().len();
        let mut text = if n == 1 {
            "1 artifact".to_owned()
        } else {
            format!("{n} artifacts")
        };
        if self.truncated {
            text.push_str(" · not all shown");
        }
        text
    }

    /// What the right side says without a selected page.
    pub fn empty_text(&self) -> &'static str {
        if self.pages.is_empty() {
            "Artifacts you publish from now on appear here."
        } else {
            "Pick an artifact."
        }
    }

    /// A row's second line: "project · Nov 8".
    pub fn row_detail(page: &Page) -> String {
        [page.project.as_str(), &dates::short(page.updated_secs)]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// The detail's subtitle: "account · project · Nov 8, 2026 14:05".
    pub fn subtitle(page: &Page) -> String {
        [
            page.account.as_str(),
            page.project.as_str(),
            &dates::long(page.updated_secs),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
    }

    /// Moves the selection through the pages shown; from a hidden selection, to the first shown.
    fn move_by(&mut self, by: i32) -> Outcome {
        let shown: Vec<String> = self.shown().iter().map(|p| p.key().to_owned()).collect();
        if shown.is_empty() {
            return Outcome::Ignored;
        }
        let at = self
            .selection
            .as_deref()
            .and_then(|key| shown.iter().position(|k| k == key));
        let next = match at {
            Some(i) => (i as i64 + i64::from(by)).clamp(0, shown.len() as i64 - 1) as usize,
            None => 0,
        };
        self.selection = Some(shown[next].clone());
        Outcome::Moved
    }

    /// Arrows move through the pages shown, Return opens the selected page, Esc clears the
    /// search, and anything else types into it.
    pub fn press(&mut self, press: &Press) -> Outcome {
        match press.key.as_str() {
            "down" => return self.move_by(1),
            "up" => return self.move_by(-1),
            "enter" => {
                return match self.selected() {
                    Some(page) => Outcome::Open(Box::new(page.clone())),
                    None => Outcome::Ignored,
                };
            }
            "escape" => {
                return if self.search.is_empty() {
                    Outcome::Ignored
                } else {
                    self.search.clear();
                    Outcome::Cleared
                };
            }
            _ => {}
        }
        if press.platform {
            return Outcome::Ignored;
        }
        match type_key(
            &mut self.search,
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
mod tests {
    use super::*;

    const NOW: f64 = 1_800_000_000.0;

    fn page(folder: &str, title: &str, project: &str, id: &str) -> Page {
        Page {
            id: id.into(),
            title: title.into(),
            folder: folder.into(),
            project: project.into(),
            account: "me@example.com".into(),
            updated_secs: NOW,
            link: if id.is_empty() {
                String::new()
            } else {
                format!("https://claude.ai/artifact/{id}")
            },
            path: format!("/pages/{folder}/index.html"),
            ..Page::default()
        }
    }

    fn three() -> Vec<Page> {
        vec![
            page("a-1", "Bridge plan", "notchling", "KVBHDXYvY8f1b1AZG6PbAR"),
            page(
                "b-2",
                "Release notes",
                "notchling",
                "7Qp2mN9xLkR4tVwYzB3cDe",
            ),
            page("c-3", "Kit audit", "apps/flutter", ""),
        ]
    }

    #[test]
    fn the_list_selects_its_first_page_and_keeps_a_selection_that_is_still_there() {
        let mut m = PagesModel::new();
        assert_eq!(m.selected(), None);
        assert_eq!(m.count_text(), "0 artifacts");
        m.set_pages(three());
        assert_eq!(m.selection.as_deref(), Some("a-1"));
        assert_eq!(m.count_text(), "3 artifacts");
        m.select("b-2");
        m.set_pages(three());
        assert_eq!(m.selection.as_deref(), Some("b-2"), "a reload keeps it");
        // Its page gone: the first shown.
        m.set_pages(vec![three()[2].clone()]);
        assert_eq!(m.selection.as_deref(), Some("c-3"));
        assert_eq!(m.count_text(), "1 artifact");
        m.set_pages(Vec::new());
        assert_eq!(m.selection, None);
    }

    #[test]
    fn a_list_cut_short_says_so_in_the_count() {
        let mut m = PagesModel::new();
        m.set_list(PageList {
            pages: three(),
            truncated: true,
        });
        assert!(m.truncated);
        assert_eq!(m.count_text(), "3 artifacts · not all shown");
        m.set_pages(three());
        assert!(!m.truncated);
    }

    #[test]
    fn the_search_narrows_the_list_by_words_or_a_pasted_link() {
        let mut m = PagesModel::new();
        m.set_pages(three());
        assert_eq!(m.shown().len(), 3);
        m.search = "notes".into();
        let shown: Vec<&str> = m.shown().iter().map(|p| p.key()).collect();
        assert_eq!(shown, ["b-2"]);
        m.search = "NOTCHLING release".into();
        assert_eq!(m.shown().len(), 1, "every word, any case, title or project");
        m.search = "flutter".into();
        assert_eq!(m.shown()[0].key(), "c-3", "the project matches too");
        m.search = "https://claude.ai/artifact/KVBHDXYvY8f1b1AZG6PbAR".into();
        assert_eq!(m.shown()[0].key(), "a-1", "a pasted link finds its page");
        m.search = "nothing like this".into();
        assert!(m.shown().is_empty());
    }

    #[test]
    fn selecting_a_hidden_page_clears_the_search_so_it_shows() {
        let mut m = PagesModel::new();
        m.set_pages(three());
        m.search = "notes".into();
        m.select("b-2");
        assert_eq!(m.search, "notes", "a shown page keeps the search");
        m.select("c-3");
        assert_eq!(m.search, "");
        assert_eq!(m.selected().map(|p| p.title.as_str()), Some("Kit audit"));
        // The selected page is read from every page, hidden or not.
        m.search = "bridge".into();
        assert_eq!(m.selected().map(|p| p.key()), Some("c-3"));
    }

    #[test]
    fn the_texts_read_like_the_swift_window() {
        let mut m = PagesModel::new();
        assert_eq!(
            m.empty_text(),
            "Artifacts you publish from now on appear here."
        );
        m.set_pages(three());
        assert_eq!(m.empty_text(), "Pick an artifact.");
        let p = &three()[0];
        assert_eq!(PagesModel::row_detail(p), "notchling · Jan 15");
        assert_eq!(
            PagesModel::subtitle(p),
            "me@example.com · notchling · Jan 15, 2027 08:00"
        );
        let mut bare = p.clone();
        bare.project.clear();
        bare.account.clear();
        assert_eq!(PagesModel::row_detail(&bare), "Jan 15");
        assert_eq!(PagesModel::subtitle(&bare), "Jan 15, 2027 08:00");
    }

    #[test]
    fn keys_move_open_clear_and_type() {
        let mut m = PagesModel::new();
        m.set_pages(three());
        assert_eq!(m.press(&Press::plain("down", None)), Outcome::Moved);
        assert_eq!(m.selection.as_deref(), Some("b-2"));
        assert_eq!(m.press(&Press::plain("down", None)), Outcome::Moved);
        assert_eq!(m.press(&Press::plain("down", None)), Outcome::Moved);
        assert_eq!(m.selection.as_deref(), Some("c-3"), "stops at the end");
        assert_eq!(m.press(&Press::plain("up", None)), Outcome::Moved);
        assert_eq!(m.selection.as_deref(), Some("b-2"));
        assert_eq!(
            m.press(&Press::plain("enter", Some("\r"))),
            Outcome::Open(Box::new(three()[1].clone()))
        );
        assert_eq!(m.press(&Press::plain("k", Some("k"))), Outcome::Typed);
        assert_eq!(m.press(&Press::plain("i", Some("i"))), Outcome::Typed);
        assert_eq!(m.search, "ki");
        assert_eq!(m.shown().len(), 1);
        // Arrows move within the pages shown: from a hidden selection, to the first shown.
        assert_eq!(m.press(&Press::plain("down", None)), Outcome::Moved);
        assert_eq!(m.selection.as_deref(), Some("c-3"));
        assert_eq!(m.press(&Press::plain("escape", None)), Outcome::Cleared);
        assert_eq!(m.search, "");
        assert_eq!(m.press(&Press::plain("escape", None)), Outcome::Ignored);
        assert_eq!(m.press(&Press::command("c")), Outcome::Ignored);
        assert_eq!(m.press(&Press::plain("backspace", None)), Outcome::Ignored);
        let mut empty = PagesModel::new();
        assert_eq!(empty.press(&Press::plain("enter", None)), Outcome::Ignored);
        assert_eq!(empty.press(&Press::plain("down", None)), Outcome::Ignored);
    }
}
