use notchling_engine::search::SessionTool;

use notchling_engine::search::facets::FacetValue;
use notchling_engine::search::query::parse::parse;

use super::chips::{chips, Known};
use super::embed_feed::{EmbedState, Meaning};
use super::model::SearchModel;
use super::wire::{Page, Results, SessionRow};
use crate::views::ViewSpec;

pub const NOW_SECS: f64 = 1_800_000_000.0;

pub const VIEWS: [ViewSpec; 3] = [
    ViewSpec {
        name: "search-panel",
        size: (680.0, 458.0),
        about: "the Search panel just opened: the newest pages and conversations (made-up data)",
    },
    ViewSpec {
        name: "search-panel-results",
        size: (680.0, 308.0),
        about:
            "the Search panel with words typed, a session selected and the words bold (made-up data)",
    },
    ViewSpec {
        name: "search-panel-facets",
        size: (680.0, 268.0),
        about: "the Search panel with chips: a tool, a time range and a project that does not exist, in red (made-up data)",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Opened,
    Results,
    Facets,
}

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        "search-panel" => Shown::Opened,
        "search-panel-results" => Shown::Results,
        "search-panel-facets" => Shown::Facets,
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
        parent: None,
        related: false,
    }
}

pub const QUERY: &str = "bridge";
pub const FACET_QUERY: &str = "tool:codex when:7d project:nosuch";

fn known() -> Known {
    let mut known = Known::default();
    known.set(
        "tool",
        ["claude", "codex", "agy"]
            .iter()
            .map(|t| FacetValue {
                value: (*t).to_owned(),
                label: (*t).to_owned(),
                count: 1,
            })
            .collect(),
    );
    known.set("project", Vec::new());
    known
}

pub fn results(shown: Shown) -> Results {
    match shown {
        Shown::Facets => Results {
            pages: Vec::new(),
            sessions: vec![
                session(
                    SessionTool::Codex,
                    "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
                    "Review the pairing panel",
                    "/Users/me/go/src/pp/notchling/apps/flutter",
                    None,
                    7_200.0,
                ),
                session(
                    SessionTool::Codex,
                    "0199c3d4-e5f6-7a8b-9c0d-1e2f3a4b5c6d",
                    "Trace the search index rebuild",
                    "/Users/me/go/src/pp/notchling/crates/engine",
                    None,
                    2.0 * 86_400.0,
                ),
            ],
            indexing: false,
            secrets: Vec::new(),
        },
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
            secrets: Vec::new(),
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
            secrets: Vec::new(),
        },
    }
}

pub fn model(shown: Shown) -> SearchModel {
    let mut m = SearchModel::new();
    if shown == Shown::Results {
        m.query = QUERY.to_owned();
    }
    if shown == Shown::Facets {
        m.query = FACET_QUERY.to_owned();
        m.chips = chips(&parse(FACET_QUERY), &known());
    }
    let mut found = results(shown);
    if shown == Shown::Results && std::env::var_os("NOTCHLING_RENDER_SEARCH_RELATED").is_some() {
        if let Some(last) = found.sessions.last_mut() {
            last.related = true;
        }
        m.meaning = Meaning {
            state: EmbedState::Indexing,
            chunks: 15_000,
            embedded: 1_240,
        };
    }
    m.apply(found, None);
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
        for item in results.items() {
            if let Item::Session(s) = item {
                let snippet = s.snippet.as_deref().unwrap();
                for (start, end) in &s.highlights {
                    assert_eq!(&snippet[*start..*end], QUERY, "{snippet}");
                }
            }
        }
        let facets = model(Shown::Facets);
        assert_eq!(facets.query, FACET_QUERY);
        assert_eq!(facets.items().len(), 2);
        let seen: Vec<(&str, bool)> = facets
            .chips
            .iter()
            .map(|c| (c.label.as_str(), c.known))
            .collect();
        assert_eq!(
            seen,
            [
                ("Tool: Codex", true),
                ("Last 7 Days", true),
                ("No Such Project", false)
            ]
        );
        assert_eq!(facets.height(), VIEWS[2].size.1);
        assert_eq!(opened.height(), VIEWS[0].size.1);
        assert_eq!(results.height(), VIEWS[1].size.1);
    }
}
