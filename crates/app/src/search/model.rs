//! What the search panel shows and what each key does, pure (the Swift `SearchModel` and
//! `SearchPanelController.handleKey`): the query, the results grouped Pages then Sessions, the
//! selection, the panel's height for what it shows, the footer's hints, and a short note such as
//! "Link Copied". The GPUI view (`view`, macOS) only lays it out and asks the daemon.

use super::wire::{Page, Results, SessionRow};
use crate::notch::answer::{type_key, Typed};

/// The panel's sizes, in points (the Swift `SearchLayout`).
pub mod layout {
    pub const WIDTH: f32 = 680.0;
    pub const FIELD: f32 = 56.0;
    pub const PAGE_ROW: f32 = 48.0;
    pub const SESSION_ROW: f32 = 54.0;
    pub const HEADER: f32 = 26.0;
    pub const LIST_PADDING: f32 = 6.0;
    pub const EMPTY: f32 = 52.0;
    pub const FOOTER: f32 = 30.0;
    /// The list scrolls beyond this height.
    pub const MAX_LIST: f32 = 8.0 * 48.0;
}

/// How long a note such as "Link Copied" stays in the footer.
pub const NOTE_MS: i64 = 1600;
/// How many of each are asked for with an empty query (the newest), and with words.
pub const EMPTY_LIMIT: usize = 6;
pub const LIMIT: usize = 30;
/// How many pages are listed at most with words typed (sessions keep the daemon's limit).
pub const PAGE_CAP: usize = 20;

/// Which group a row is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Pages,
    Sessions,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Pages => "Pages",
            Section::Sessions => "Sessions",
        }
    }
}

/// One row of the results.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Page(Page),
    Session(SessionRow),
}

impl Item {
    /// Stable across searches, so the selection can stay on the same row.
    pub fn id(&self) -> String {
        match self {
            Item::Page(p) => format!("page:{}", p.key()),
            Item::Session(s) => format!("session:{}", s.id),
        }
    }

    pub fn section(&self) -> Section {
        match self {
            Item::Page(_) => Section::Pages,
            Item::Session(_) => Section::Sessions,
        }
    }

    pub fn row_height(&self) -> f32 {
        match self {
            Item::Page(_) => layout::PAGE_ROW,
            Item::Session(_) => layout::SESSION_ROW,
        }
    }
}

/// A key press, as GPUI reports it, with the modifiers that matter here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Press {
    pub key: String,
    pub key_char: Option<String>,
    /// ⌘.
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

/// What a key press asks the view to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The selection moved.
    Moved,
    /// Return: open the selected row.
    Open(Item),
    /// Close the panel; `returning` gives the app you were in its focus back.
    Hide { returning: bool },
    /// Esc with words typed: the query was cleared (search again).
    Cleared,
    /// The query changed (search again).
    Typed,
    /// ⌘C: put `text` on the clipboard and show `note`.
    Copy { text: String, note: &'static str },
    Ignored,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchModel {
    pub query: String,
    items: Vec<Item>,
    pub selected: usize,
    /// A search is out for the current query.
    pub searching: bool,
    /// Why the last search failed, if it did.
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

    /// How many of each to ask for: a few of the newest with no words, more with words.
    pub fn limit(&self) -> usize {
        todo!()
    }

    /// Takes a search's answer: pages first (capped), then sessions. The selection stays on
    /// `keeping`'s row when it is still there, else goes to the top.
    pub fn apply(&mut self, results: Results, keeping: Option<&str>) {
        todo!()
    }

    /// Whether the row at `index` is the first of its group, which gets a header above it.
    pub fn starts_section(&self, index: usize) -> bool {
        todo!()
    }

    /// The rows and their headers, up to the height the list scrolls beyond.
    pub fn list_height(&self) -> f32 {
        todo!()
    }

    /// The panel's height for what it shows now.
    pub fn height(&self) -> f32 {
        todo!()
    }

    pub fn move_by(&mut self, by: i32) {
        todo!()
    }

    /// What the empty list says.
    pub fn empty_text(&self) -> &'static str {
        todo!()
    }

    /// Shows `text` in the footer for a moment.
    pub fn flash(&mut self, text: &str, now_ms: i64) {
        todo!()
    }

    /// The note, while it lasts.
    pub fn note(&self, now_ms: i64) -> Option<&str> {
        todo!()
    }

    /// The footer's left side: the note, else the error, else what the panel searches.
    pub fn footer_text(&self, now_ms: i64) -> String {
        todo!()
    }

    /// The footer's key hints for the selected row: (key, what it does).
    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        todo!()
    }

    /// Arrows move, Return opens, ⌘C copies the link or session id, ⌘W closes, Esc clears then
    /// closes, and anything else types into the query.
    pub fn press(&mut self, press: &Press) -> Outcome {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::search::SessionTool;

    const NOW: i64 = 1_800_000_000_000;

    fn page(folder: &str, link: &str) -> Page {
        Page {
            id: link.rsplit('/').next().unwrap_or("").to_owned(),
            title: format!("Page {folder}"),
            folder: folder.into(),
            project: "notchling".into(),
            account: "me@example.com".into(),
            updated_secs: 1_800_000_000.0,
            link: link.into(),
            path: format!("{folder}/index.html"),
        }
    }

    fn session(id: &str, snippet: Option<&str>) -> SessionRow {
        SessionRow {
            id: format!("claude:{id}"),
            tool: SessionTool::Claude,
            session_id: id.into(),
            title: format!("Session {id}"),
            folder: "/work/nl".into(),
            snippet: snippet.map(str::to_owned),
            highlights: Vec::new(),
            updated_secs: 1_800_000_000.0,
        }
    }

    fn results(pages: usize, sessions: usize) -> Results {
        Results {
            pages: (0..pages)
                .map(|i| page(&format!("/p/{i}"), &format!("https://claude.ai/artifact/id{i}")))
                .collect(),
            sessions: (0..sessions)
                .map(|i| session(&format!("s{i}"), Some("a match here")))
                .collect(),
        }
    }

    #[test]
    fn results_are_grouped_pages_then_sessions_with_headers_at_each_start() {
        let mut m = SearchModel::new();
        assert!(m.items().is_empty());
        assert_eq!(m.current(), None);
        m.apply(results(2, 2), None);
        let sections: Vec<Section> = m.items().iter().map(Item::section).collect();
        assert_eq!(
            sections,
            [Section::Pages, Section::Pages, Section::Sessions, Section::Sessions]
        );
        assert!(m.starts_section(0));
        assert!(!m.starts_section(1));
        assert!(m.starts_section(2));
        assert!(!m.starts_section(3));
        assert!(!m.starts_section(9));
        assert_eq!(m.selected, 0);
        assert_eq!(Section::Pages.title(), "Pages");
        assert_eq!(Section::Sessions.title(), "Sessions");
    }

    #[test]
    fn the_limit_and_the_page_cap_follow_the_query() {
        let mut m = SearchModel::new();
        assert_eq!(m.limit(), EMPTY_LIMIT);
        m.apply(results(10, 0), None);
        assert_eq!(m.items().len(), EMPTY_LIMIT, "no words: a few of the newest");
        m.query = "bridge".into();
        assert_eq!(m.limit(), LIMIT);
        m.apply(results(25, 3), None);
        assert_eq!(m.items().len(), PAGE_CAP + 3);
    }

    #[test]
    fn the_selection_stays_on_the_same_row_when_it_is_still_there() {
        let mut m = SearchModel::new();
        m.apply(results(2, 2), None);
        m.move_by(2);
        let kept = m.current().unwrap().id();
        assert_eq!(kept, "session:claude:s0");
        // A new answer with one page fewer: the same session row is now third.
        m.apply(results(1, 2), Some(&kept));
        assert_eq!(m.selected, 1);
        assert_eq!(m.current().unwrap().id(), kept);
        // Gone: back to the top.
        m.apply(results(1, 0), Some(&kept));
        assert_eq!(m.selected, 0);
    }

    #[test]
    fn moving_stays_inside_the_list() {
        let mut m = SearchModel::new();
        m.move_by(1);
        assert_eq!(m.selected, 0, "nothing to move in");
        m.apply(results(1, 2), None);
        m.move_by(-1);
        assert_eq!(m.selected, 0);
        m.move_by(5);
        assert_eq!(m.selected, 2);
        m.move_by(-1);
        assert_eq!(m.selected, 1);
    }

    #[test]
    fn the_height_follows_the_rows_their_headers_and_the_cap() {
        let m = SearchModel::new();
        let chrome = layout::FIELD + 1.0 + layout::FOOTER;
        assert_eq!(m.list_height(), 0.0);
        assert_eq!(m.height(), chrome + layout::EMPTY + 1.0);
        let mut m = SearchModel::new();
        m.apply(results(2, 1), None);
        let list = layout::HEADER + 2.0 * layout::PAGE_ROW + layout::HEADER + layout::SESSION_ROW;
        assert_eq!(m.list_height(), list);
        assert_eq!(m.height(), chrome + list + 2.0 * layout::LIST_PADDING + 1.0);
        m.query = "x".into();
        m.apply(results(20, 10), None);
        assert_eq!(m.list_height(), layout::MAX_LIST, "scrolls beyond this");
    }

    #[test]
    fn the_empty_text_says_why_the_list_is_empty() {
        let mut m = SearchModel::new();
        assert_eq!(m.empty_text(), "Nothing Saved Yet");
        m.query = "zzz".into();
        m.searching = true;
        assert_eq!(m.empty_text(), "Searching…");
        m.searching = false;
        assert_eq!(m.empty_text(), "No Matches");
    }

    #[test]
    fn a_note_shows_for_a_moment_over_the_footers_text() {
        let mut m = SearchModel::new();
        assert_eq!(m.footer_text(NOW), "Pages and Sessions");
        m.flash("Link Copied", NOW);
        assert_eq!(m.note(NOW), Some("Link Copied"));
        assert_eq!(m.footer_text(NOW + NOTE_MS - 1), "Link Copied");
        assert_eq!(m.note(NOW + NOTE_MS), None);
        assert_eq!(m.footer_text(NOW + NOTE_MS), "Pages and Sessions");
        m.error = Some("the daemon isn't attached".into());
        assert_eq!(m.footer_text(NOW + NOTE_MS), "the daemon isn't attached");
    }

    #[test]
    fn the_hints_name_what_return_and_copy_do_for_the_row() {
        let mut m = SearchModel::new();
        assert_eq!(m.hints(), vec![("esc", "Close")]);
        m.apply(results(1, 1), None);
        assert_eq!(
            m.hints(),
            vec![("↩", "Open"), ("⌘C", "Copy Link"), ("esc", "Close")]
        );
        m.move_by(1);
        assert_eq!(
            m.hints(),
            vec![("↩", "Open in Chat"), ("⌘C", "Copy Session ID"), ("esc", "Close")]
        );
        // A page with no link has nothing to copy.
        let mut m = SearchModel::new();
        m.apply(
            Results {
                pages: vec![page("/p", "")],
                sessions: vec![],
            },
            None,
        );
        assert_eq!(m.hints(), vec![("↩", "Open"), ("esc", "Close")]);
    }

    #[test]
    fn arrows_return_and_escape_do_what_the_swift_panel_does() {
        let mut m = SearchModel::new();
        m.apply(results(1, 1), None);
        assert_eq!(m.press(&Press::plain("down", None)), Outcome::Moved);
        assert_eq!(m.selected, 1);
        assert_eq!(m.press(&Press::plain("up", None)), Outcome::Moved);
        assert_eq!(m.selected, 0);
        assert_eq!(
            m.press(&Press::plain("enter", Some("\r"))),
            Outcome::Open(m.items()[0].clone())
        );
        // Esc clears words first, then closes and gives focus back.
        m.query = "bri".into();
        assert_eq!(m.press(&Press::plain("escape", None)), Outcome::Cleared);
        assert_eq!(m.query, "");
        assert_eq!(
            m.press(&Press::plain("escape", None)),
            Outcome::Hide { returning: true }
        );
        // Return with nothing selected does nothing.
        let mut empty = SearchModel::new();
        assert_eq!(empty.press(&Press::plain("enter", None)), Outcome::Ignored);
    }

    #[test]
    fn command_keys_copy_and_close_and_never_type() {
        let mut m = SearchModel::new();
        m.apply(results(1, 1), None);
        assert_eq!(
            m.press(&Press::command("c")),
            Outcome::Copy {
                text: "https://claude.ai/artifact/id0".into(),
                note: "Link Copied"
            }
        );
        m.move_by(1);
        assert_eq!(
            m.press(&Press::command("c")),
            Outcome::Copy {
                text: "s0".into(),
                note: "Session ID Copied"
            }
        );
        assert_eq!(
            m.press(&Press::command("w")),
            Outcome::Hide { returning: true }
        );
        assert_eq!(m.press(&Press::command("v")), Outcome::Ignored);
        assert_eq!(m.query, "");
        // ⌘C on a page without a link, or with no row: nothing.
        let mut m = SearchModel::new();
        assert_eq!(m.press(&Press::command("c")), Outcome::Ignored);
        m.apply(
            Results {
                pages: vec![page("/p", "")],
                sessions: vec![],
            },
            None,
        );
        assert_eq!(m.press(&Press::command("c")), Outcome::Ignored);
    }

    #[test]
    fn plain_keys_type_into_the_query() {
        let mut m = SearchModel::new();
        assert_eq!(m.press(&Press::plain("b", Some("b"))), Outcome::Typed);
        assert_eq!(m.press(&Press::plain("space", Some(" "))), Outcome::Typed);
        assert_eq!(m.press(&Press::plain("é", Some("é"))), Outcome::Typed);
        assert_eq!(m.query, "b é");
        assert_eq!(m.press(&Press::plain("backspace", None)), Outcome::Typed);
        assert_eq!(m.query, "b ");
        assert_eq!(m.press(&Press::plain("tab", Some("\t"))), Outcome::Ignored);
        assert_eq!(m.press(&Press::plain("left", None)), Outcome::Ignored);
        let mut empty = SearchModel::new();
        assert_eq!(
            empty.press(&Press::plain("backspace", None)),
            Outcome::Ignored
        );
        // Control and Option chords are not typing either.
        let mut m = SearchModel::new();
        let mut ctrl = Press::plain("a", Some("a"));
        ctrl.control = true;
        assert_eq!(m.press(&ctrl), Outcome::Ignored);
        let mut alt = Press::plain("a", Some("å"));
        alt.alt = true;
        assert_eq!(m.press(&alt), Outcome::Typed, "Option types its character");
        assert_eq!(m.query, "å");
    }

    #[test]
    fn item_ids_tell_pages_and_sessions_apart() {
        let p = Item::Page(page("/p/1", ""));
        let s = Item::Session(session("abc", None));
        assert_eq!(p.id(), "page:/p/1");
        assert_eq!(s.id(), "session:claude:abc");
        assert_eq!(p.row_height(), layout::PAGE_ROW);
        assert_eq!(s.row_height(), layout::SESSION_ROW);
        let _ = Typed::Ignored;
    }
}
