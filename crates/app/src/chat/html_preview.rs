use std::path::{Component, Path, PathBuf};

use crate::chat::file_panel::{self, Body, Stamp};
use crate::pages::local::PageSource;

pub const GONE: &str = "This file is gone.";

pub fn is_html(path: &Path) -> bool {
    matches!(
        path.extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .as_deref(),
        Some("html" | "htm" | "xhtml")
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Look {
    #[default]
    Page,
    Source,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HtmlView {
    pub path: PathBuf,
    pub look: Look,
    pub stamp: Stamp,
    pub changes: bool,
    pub page_subtitle: String,
}

impl HtmlView {
    pub fn file(path: PathBuf, home: Option<&Path>) -> HtmlView {
        HtmlView::new(path, Look::Page, false, home)
    }

    pub fn git(path: PathBuf, look: Look, home: Option<&Path>) -> HtmlView {
        HtmlView::new(path, look, true, home)
    }

    fn new(path: PathBuf, look: Look, changes: bool, home: Option<&Path>) -> HtmlView {
        HtmlView {
            stamp: file_panel::stamp(&path),
            page_subtitle: page_subtitle(&path, home),
            path,
            look,
            changes,
        }
    }

    pub fn is_page(&self) -> bool {
        self.look == Look::Page
    }

    pub fn moved(&mut self) -> bool {
        let now = file_panel::stamp(&self.path);
        if now == self.stamp {
            return false;
        }
        self.stamp = now;
        true
    }

    pub fn page_tip(&self) -> &'static str {
        "Show as Page"
    }

    pub fn other_tip(&self) -> &'static str {
        if self.changes {
            "Show Changes"
        } else {
            "Show Source"
        }
    }
}

pub fn source(path: &Path) -> Result<PageSource, String> {
    if !path.is_file() {
        return Err(GONE.to_owned());
    }
    let folder = path.parent().ok_or(GONE)?.to_path_buf();
    Ok(PageSource::File {
        file: path.to_path_buf(),
        folder,
    })
}

pub fn page_subtitle(path: &Path, home: Option<&Path>) -> String {
    let whole = Body::Text {
        text: String::new(),
        cut: false,
    };
    format!(
        "{} \u{b7} Scripts off",
        file_panel::subtitle(path, home, &whole)
    )
}

pub fn git_look(status: &str) -> Look {
    match status {
        "?" | "A" => Look::Page,
        _ => Look::Source,
    }
}

pub fn git_offers(status: &str) -> bool {
    status != "D"
}

pub fn repo_root(folder: &Path) -> Option<PathBuf> {
    folder
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

pub fn git_file(folder: &Path, relative: &str) -> Option<PathBuf> {
    let relative = Path::new(relative);
    if !is_html(relative)
        || !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let found = repo_root(folder)?.join(relative);
    found.is_file().then_some(found)
}
