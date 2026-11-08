//! What the Artifacts window shows, pure (the Swift `PagesModel`): every saved page, the ones the
//! search leaves, the selected one, and what each key does in the list. The GPUI view (`view`,
//! macOS) only lays it out and asks the daemon for the list.

use notchling_engine::search::{find_pages, PageEntry};

use super::dates;
use crate::notch::answer::{type_key, Typed};
use crate::search::model::Press;
use crate::search::wire::Page;

/// What a key press asks the view to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The selection moved.
    Moved,
    /// Return: open the selected page in its own window.
    Open(Page),
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
        todo!()
    }

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// The pages the search leaves: every word in the title, project, account or link, or the
    /// page a pasted link or id names; every page without words.
    pub fn shown(&self) -> Vec<&Page> {
        todo!()
    }

    /// The selected page, listed or not.
    pub fn selected(&self) -> Option<&Page> {
        todo!()
    }

    /// Selects a page by key; a page the search hides clears the search first, so it shows.
    pub fn select(&mut self, key: &str) {
        todo!()
    }

    /// "1 artifact", "12 artifacts".
    pub fn count_text(&self) -> String {
        todo!()
    }

    /// What the right side says without a selected page.
    pub fn empty_text(&self) -> &'static str {
        todo!()
    }

    /// A row's second line: "project · Nov 8".
    pub fn row_detail(page: &Page) -> String {
        todo!()
    }

    /// The detail's subtitle: "account · project · Nov 8, 2026 14:05".
    pub fn subtitle(page: &Page) -> String {
        todo!()
    }

    /// Arrows move through the pages shown, Return opens the selected page, Esc clears the
    /// search, and anything else types into it.
    pub fn press(&mut self, press: &Press) -> Outcome {
        todo!()
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
            path: format!("{folder}/index.html"),
        }
    }

    fn three() -> Vec<Page> {
        vec![
            page("/p/a", "Bridge plan", "notchling", "KVBHDXYvY8f1b1AZG6PbAR"),
            page("/p/b", "Release notes", "notchling", "7Qp2mN9xLkR4tVwYzB3cDe"),
            page("/p/c", "Kit audit", "apps/flutter", ""),
        ]
    }

    #[test]
    fn the_list_selects_its_first_page_and_keeps_a_selection_that_is_still_there() {
        let mut m = PagesModel::new();
        assert_eq!(m.selected(), None);
        assert_eq!(m.count_text(), "0 artifacts");
        m.set_pages(three());
        assert_eq!(m.selection.as_deref(), Some("/p/a"));
        assert_eq!(m.count_text(), "3 artifacts");
        m.select("/p/b");
        m.set_pages(three());
        assert_eq!(m.selection.as_deref(), Some("/p/b"), "a reload keeps it");
        // Its page gone: the first shown.
        m.set_pages(vec![three()[2].clone()]);
        assert_eq!(m.selection.as_deref(), Some("/p/c"));
        assert_eq!(m.count_text(), "1 artifact");
        m.set_pages(Vec::new());
        assert_eq!(m.selection, None);
    }

    #[test]
    fn the_search_narrows_the_list_by_words_or_a_pasted_link() {
        let mut m = PagesModel::new();
        m.set_pages(three());
        assert_eq!(m.shown().len(), 3);
        m.search = "notes".into();
        let shown: Vec<&str> = m.shown().iter().map(|p| p.key()).collect();
        assert_eq!(shown, ["/p/b"]);
        m.search = "NOTCHLING release".into();
        assert_eq!(m.shown().len(), 1, "every word, any case, title or project");
        m.search = "flutter".into();
        assert_eq!(m.shown()[0].key(), "/p/c", "the project matches too");
        m.search = "https://claude.ai/artifact/KVBHDXYvY8f1b1AZG6PbAR".into();
        assert_eq!(m.shown()[0].key(), "/p/a", "a pasted link finds its page");
        m.search = "nothing like this".into();
        assert!(m.shown().is_empty());
    }

    #[test]
    fn selecting_a_hidden_page_clears_the_search_so_it_shows() {
        let mut m = PagesModel::new();
        m.set_pages(three());
        m.search = "notes".into();
        m.select("/p/b");
        assert_eq!(m.search, "notes", "a shown page keeps the search");
        m.select("/p/c");
        assert_eq!(m.search, "");
        assert_eq!(m.selected().map(|p| p.title.as_str()), Some("Kit audit"));
        // The selected page is read from every page, hidden or not.
        m.search = "bridge".into();
        assert_eq!(m.selected().map(|p| p.key()), Some("/p/c"));
    }

    #[test]
    fn the_texts_read_like_the_swift_window() {
        let mut m = PagesModel::new();
        assert_eq!(m.empty_text(), "Artifacts you publish from now on appear here.");
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
        assert_eq!(m.selection.as_deref(), Some("/p/b"));
        assert_eq!(m.press(&Press::plain("down", None)), Outcome::Moved);
        assert_eq!(m.press(&Press::plain("down", None)), Outcome::Moved);
        assert_eq!(m.selection.as_deref(), Some("/p/c"), "stops at the end");
        assert_eq!(m.press(&Press::plain("up", None)), Outcome::Moved);
        assert_eq!(m.selection.as_deref(), Some("/p/b"));
        assert_eq!(
            m.press(&Press::plain("enter", Some("\r"))),
            Outcome::Open(three()[1].clone())
        );
        assert_eq!(m.press(&Press::plain("k", Some("k"))), Outcome::Typed);
        assert_eq!(m.press(&Press::plain("i", Some("i"))), Outcome::Typed);
        assert_eq!(m.search, "ki");
        assert_eq!(m.shown().len(), 1);
        // Arrows move within the pages shown: from a hidden selection, to the first shown.
        assert_eq!(m.press(&Press::plain("down", None)), Outcome::Moved);
        assert_eq!(m.selection.as_deref(), Some("/p/c"));
        assert_eq!(m.press(&Press::plain("escape", None)), Outcome::Cleared);
        assert_eq!(m.search, "");
        assert_eq!(m.press(&Press::plain("escape", None)), Outcome::Ignored);
        assert_eq!(m.press(&Press::command("c")), Outcome::Ignored);
        assert_eq!(m.press(&Press::plain("backspace", None)), Outcome::Ignored);
        let mut empty = PagesModel::new();
        assert_eq!(empty.press(&Press::plain("enter", None)), Outcome::Ignored);
        assert_eq!(empty.press(&Press::plain("down", None)), Outcome::Ignored);
        let _ = Typed::Ignored;
    }
}
