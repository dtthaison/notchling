//! The Artifacts window for `--render pages-window` (part U10): made-up pages, one selected,
//! never the real library.

use super::model::PagesModel;
use crate::search::wire::Page;
use crate::views::ViewSpec;

/// The fixed clock the render draws at (seconds since 1970).
pub const NOW_SECS: f64 = 1_800_000_000.0;

pub const NAME: &str = "pages-window";

pub const VIEWS: [ViewSpec; 1] = [ViewSpec {
    name: NAME,
    size: (1080.0, 720.0),
    about: "the Artifacts window: the library list with a page selected (made-up data)",
}];

fn page(id: &str, title: &str, project: &str, account: &str, age_secs: f64) -> Page {
    Page {
        id: id.into(),
        title: title.into(),
        folder: format!("/Users/me/Library/Application Support/Notchling/pages/{id}"),
        project: project.into(),
        account: account.into(),
        updated_secs: NOW_SECS - age_secs,
        link: if id.starts_with("local") {
            String::new()
        } else {
            format!("https://claude.ai/artifact/{id}")
        },
        path: format!("/Users/me/Library/Application Support/Notchling/pages/{id}/index.html"),
    }
}

/// The pages the render lists.
pub fn pages() -> Vec<Page> {
    vec![
        page("KVBHDXYvY8f1b1AZG6PbAR", "Phone bridge: pairing flow", "notchling", "me@example.com", 1_800.0),
        page("7Qp2mN9xLkR4tVwYzB3cDe", "Release notes draft", "notchling", "me@example.com", 86_400.0),
        page("Hs8fJ2kLm3nP4qR5tU6vW7", "Flutter kit audit", "apps/flutter", "team@example.com", 3.5 * 86_400.0),
        page("local-9f3e", "Worktree cleanup checklist", "notchling", "", 9.0 * 86_400.0),
        page("Zx1cV2bN3mA4sD5fG6hJ7k", "Milestone 5 plan, as a page", "notchling", "me@example.com", 12.0 * 86_400.0),
    ]
}

/// The model the render draws: every page, the second selected.
pub fn model() -> PagesModel {
    let mut m = PagesModel::new();
    m.set_pages(pages());
    if let Some(second) = pages().get(1) {
        m.select(second.key());
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_has_its_data() {
        assert!(crate::views::spec(NAME).is_some());
        let m = model();
        assert_eq!(m.pages().len(), 5);
        assert_eq!(m.shown().len(), 5);
        assert_eq!(m.selected().map(|p| p.title.as_str()), Some("Release notes draft"));
        assert!(pages().iter().any(|p| p.link.is_empty()), "one page never got a link");
    }
}
