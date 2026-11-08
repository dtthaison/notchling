//! The search panel's views for `--render` (part U10): the panel just opened, with the newest
//! pages and conversations, and the panel with words typed and a match selected. Made-up pages and
//! sessions, never the real library or the daemon.

use notchling_engine::search::SessionTool;

use super::model::SearchModel;
use super::wire::{Page, Results, SessionRow};
use crate::views::ViewSpec;

/// The fixed clock every render draws at (seconds since 1970).
pub const NOW_SECS: f64 = 1_800_000_000.0;

/// Each window is exactly the panel's height for its data (`SearchModel::height`), as the live
/// panel's is.
pub const VIEWS: [ViewSpec; 2] = [
    ViewSpec {
        name: "search-panel",
        size: (680.0, 458.0),
        about: "the ⌘⇧P panel just opened: the newest pages and conversations (made-up data)",
    },
    ViewSpec {
        name: "search-panel-results",
        size: (680.0, 308.0),
        about:
            "the ⌘⇧P panel with words typed, a session selected and the words bold (made-up data)",
    },
];

/// What a panel render shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// Just opened: no words, the newest of each.
    Opened,
    /// "bridge" typed: the matches, the first session selected.
    Results,
}

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        "search-panel" => Shown::Opened,
        "search-panel-results" => Shown::Results,
        _ => return None,
    })
}

fn page(id: &str, title: &str, project: &str, age_secs: f64) -> Page {
    let folder = format!("{}-{}", title.to_lowercase().replace(' ', "-"), &id[..4]);
    Page {
        id: id.into(),
        title: title.into(),
        path: format!("/Users/me/Library/Application Support/Notchling/pages/{folder}/index.html"),
        folder,
        file: "index.html".into(),
        project: project.into(),
        account: "me@example.com".into(),
        updated_secs: NOW_SECS - age_secs,
        link: format!("https://claude.ai/artifact/{id}"),
        ..Page::default()
    }
}

fn session(
    tool: SessionTool,
    id: &str,
    title: &str,
    folder: &str,
    snippet: Option<(&str, &[(usize, usize)])>,
    age_secs: f64,
) -> SessionRow {
    SessionRow {
        id: format!("{}:{id}", tool.key()),
        tool,
        session_id: id.into(),
        title: title.into(),
        folder: folder.into(),
        snippet: snippet.map(|(s, _)| s.to_owned()),
        highlights: snippet.map(|(_, h)| h.to_vec()).unwrap_or_default(),
        updated_secs: NOW_SECS - age_secs,
    }
}

/// The words a results render has typed.
pub const QUERY: &str = "bridge";

/// The answer a render draws.
pub fn results(shown: Shown) -> Results {
    match shown {
        Shown::Opened => Results {
            pages: vec![
                page(
                    "KVBHDXYvY8f1b1AZG6PbAR",
                    "Phone bridge: pairing flow",
                    "notchling",
                    1_800.0,
                ),
                page(
                    "7Qp2mN9xLkR4tVwYzB3cDe",
                    "Release notes draft",
                    "notchling",
                    86_400.0,
                ),
                page(
                    "Hs8fJ2kLm3nP4qR5tU6vW7",
                    "Flutter kit audit",
                    "apps/flutter",
                    3.5 * 86_400.0,
                ),
            ],
            sessions: vec![
                session(
                    SessionTool::Claude,
                    "3f1c2a9e-7b4d-4e1f-9c8a-1d2e3f4a5b6c",
                    "Fix the bridge handshake deadline",
                    "/Users/me/go/src/pp/notchling",
                    None,
                    600.0,
                ),
                session(
                    SessionTool::Codex,
                    "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
                    "Review the pairing panel",
                    "/Users/me/go/src/pp/notchling/apps/flutter",
                    None,
                    7_200.0,
                ),
                session(
                    SessionTool::Antigravity,
                    "agy-5e6f",
                    "Second opinion on the search index",
                    "/Users/me/go/src/pp/notchling/crates/engine",
                    None,
                    2.0 * 86_400.0,
                ),
            ],
            indexing: false,
        },
        Shown::Results => Results {
            pages: vec![page(
                "KVBHDXYvY8f1b1AZG6PbAR",
                "Phone bridge: pairing flow",
                "notchling",
                1_800.0,
            )],
            sessions: vec![
                session(
                    SessionTool::Claude,
                    "3f1c2a9e-7b4d-4e1f-9c8a-1d2e3f4a5b6c",
                    "Fix the bridge handshake deadline",
                    "/Users/me/go/src/pp/notchling",
                    Some((
                        "…the bridge was off at every daemon start, so the bridge switch…",
                        &[(7, 13), (52, 58)],
                    )),
                    600.0,
                ),
                session(
                    SessionTool::Codex,
                    "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
                    "Review the pairing panel",
                    "/Users/me/go/src/pp/notchling/apps/flutter",
                    Some(("Pairing goes through the bridge on port 47821", &[(25, 31)])),
                    7_200.0,
                ),
            ],
            indexing: false,
        },
    }
}

/// The model a render starts from: the words typed and the answer applied, a session selected in
/// the results view.
pub fn model(shown: Shown) -> SearchModel {
    let mut m = SearchModel::new();
    if shown == Shown::Results {
        m.query = QUERY.to_owned();
    }
    m.apply(results(shown), None);
    if shown == Shown::Results {
        m.move_by(1);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::model::{Item, Section};

    #[test]
    fn every_view_has_its_data() {
        for spec in VIEWS {
            assert!(shown(spec.name).is_some(), "{}", spec.name);
            assert!(crate::views::spec(spec.name).is_some(), "{}", spec.name);
        }
        assert_eq!(shown("chat"), None);
    }

    #[test]
    fn the_views_show_what_their_names_say() {
        let opened = model(Shown::Opened);
        assert_eq!(opened.query, "");
        assert_eq!(opened.items().len(), 6);
        assert_eq!(opened.selected, 0);
        let results = model(Shown::Results);
        assert_eq!(results.query, QUERY);
        assert_eq!(results.items().len(), 3);
        assert_eq!(results.selected, 1);
        assert_eq!(
            results.current().map(Item::section),
            Some(Section::Sessions)
        );
        // The highlights point at the word in each snippet.
        for item in results.items() {
            if let Item::Session(s) = item {
                let snippet = s.snippet.as_deref().unwrap();
                for (start, end) in &s.highlights {
                    assert_eq!(&snippet[*start..*end], QUERY, "{snippet}");
                }
            }
        }
        // Each render window is the panel's own height.
        assert_eq!(opened.height(), VIEWS[0].size.1);
        assert_eq!(results.height(), VIEWS[1].size.1);
    }
}
