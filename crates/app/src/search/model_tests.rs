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
        path: format!("/pages/{folder}/index.html"),
        ..Page::default()
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
            .map(|i| {
                page(
                    &format!("p-{i}"),
                    &format!("https://claude.ai/artifact/id{i}"),
                )
            })
            .collect(),
        sessions: (0..sessions)
            .map(|i| session(&format!("s{i}"), Some("a match here")))
            .collect(),
        indexing: false,
        secrets: Vec::new(),
    }
}

fn only_pages(pages: Vec<Page>) -> Results {
    Results {
        pages,
        sessions: Vec::new(),
        indexing: false,
        secrets: Vec::new(),
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
        [
            Section::Pages,
            Section::Pages,
            Section::Sessions,
            Section::Sessions
        ]
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
    assert_eq!(
        m.items().len(),
        EMPTY_LIMIT,
        "no words: a few of the newest"
    );
    m.query = "bridge".into();
    assert_eq!(m.limit(), LIMIT);
    m.apply(results(25, 3), None);
    assert_eq!(m.items().len(), PAGE_CAP + 3);
    m.query = "   ".into();
    assert_eq!(m.limit(), EMPTY_LIMIT, "spaces are no words");
}

#[test]
fn the_selection_stays_on_the_same_row_when_it_is_still_there() {
    let mut m = SearchModel::new();
    m.apply(results(2, 2), None);
    m.move_by(2);
    let kept = m.current().unwrap().id();
    assert_eq!(kept, "session:claude:s0");
    m.apply(results(1, 2), Some(&kept));
    assert_eq!(m.selected, 1);
    assert_eq!(m.current().unwrap().id(), kept);
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
    m.indexing = true;
    assert_eq!(m.empty_text(), INDEXING);
    m.indexing = false;
    m.query = "zzz".into();
    m.searching = true;
    assert_eq!(m.empty_text(), "Searching…");
    m.searching = false;
    assert_eq!(m.empty_text(), "No Matches");
    m.indexing = true;
    assert_eq!(m.empty_text(), "Searching…", "the index may still find it");
    m.error = Some("detached".into());
    assert_eq!(m.empty_text(), "Search Unavailable", "the footer says why");
}

#[test]
fn a_note_shows_for_a_moment_over_the_footers_text() {
    let mut m = SearchModel::new();
    assert_eq!(m.footer_text(NOW), "Pages, Vault and Sessions");
    m.flash("Link Copied", NOW);
    assert_eq!(m.note(NOW), Some("Link Copied"));
    assert_eq!(m.footer_text(NOW + NOTE_MS - 1), "Link Copied");
    assert_eq!(m.note(NOW + NOTE_MS), None);
    assert_eq!(m.footer_text(NOW + NOTE_MS), "Pages, Vault and Sessions");
    m.indexing = true;
    assert_eq!(m.footer_text(NOW + NOTE_MS), INDEXING);
    m.error = Some("the daemon isn't attached".into());
    assert_eq!(m.footer_text(NOW + NOTE_MS), "the daemon isn't attached");
}

#[test]
fn indexing_comes_with_the_answer() {
    let mut m = SearchModel::new();
    let mut r = results(1, 0);
    r.indexing = true;
    m.apply(r, None);
    assert!(m.indexing);
    m.apply(results(1, 0), None);
    assert!(!m.indexing);
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
        vec![
            ("↩", "Open in Chat"),
            ("⌘C", "Copy Session ID"),
            ("esc", "Close")
        ]
    );
    let mut m = SearchModel::new();
    m.apply(only_pages(vec![page("p", "")]), None);
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
    m.query = "bri".into();
    assert_eq!(m.press(&Press::plain("escape", None)), Outcome::Cleared);
    assert_eq!(m.query, "");
    assert_eq!(
        m.press(&Press::plain("escape", None)),
        Outcome::Hide { returning: true }
    );
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
    let mut m = SearchModel::new();
    assert_eq!(m.press(&Press::command("c")), Outcome::Ignored);
    m.apply(only_pages(vec![page("p", "")]), None);
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
    let mut m = SearchModel::new();
    let mut ctrl = Press::plain("a", Some("a"));
    ctrl.control = true;
    assert_eq!(m.press(&ctrl), Outcome::Ignored);
    let mut alt = Press::plain("a", Some("å"));
    alt.alt = true;
    assert_eq!(m.press(&alt), Outcome::Typed);
    assert_eq!(m.query, "å");
}

#[test]
fn item_ids_tell_pages_and_sessions_apart() {
    let p = Item::Page(page("p-1", ""));
    let s = Item::Session(session("abc", None));
    assert_eq!(p.id(), "page:p-1");
    assert_eq!(s.id(), "session:claude:abc");
    assert_eq!(p.row_height(), layout::PAGE_ROW);
    assert_eq!(s.row_height(), layout::SESSION_ROW);
}

#[test]
fn vault_entries_come_between_pages_and_sessions_and_return_edits_one() {
    use notchling_engine::vault::{Kind, VaultEntry};
    let mut m = SearchModel::new();
    m.query = "github".into();
    let mut r = results(1, 1);
    let mut token = VaultEntry::new("m1-github-issues", Kind::Token);
    token.url = Some("https://github.com".into());
    r.secrets = vec![token.clone()];
    m.apply(r, None);
    let sections: Vec<Section> = m.items().iter().map(Item::section).collect();
    assert_eq!(
        sections,
        [Section::Pages, Section::Vault, Section::Sessions]
    );
    assert!(m.starts_section(1) && m.starts_section(2));
    assert_eq!(Section::Vault.title(), "Vault");
    m.move_by(1);
    assert_eq!(
        m.current().map(Item::id).as_deref(),
        Some("secret:m1-github-issues")
    );
    assert_eq!(m.hints()[0], ("↩", "Edit"));
    assert_eq!(
        m.press(&Press::plain("enter", None)),
        Outcome::Open(Item::Secret(token))
    );
    assert_eq!(
        m.press(&Press::command("c")),
        Outcome::Copy {
            text: "m1-github-issues".into(),
            note: "Name Copied",
        }
    );
    assert_eq!(m.footer_text(NOW), "Pages, Vault and Sessions");
}
