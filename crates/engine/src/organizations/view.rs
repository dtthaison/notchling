use super::folders::{self, FolderMatch};
use super::kinds::Kind;
use super::record::{Document, Organization, PERSONAL_ID};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Field,
    Shadow,
    Default,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub id: String,
    pub source: Source,
    pub known: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gap {
    pub kind: Kind,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Missing,
    Loaded(Document),
    Unreadable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    doc: Document,
    closed: Option<String>,
}

impl View {
    pub fn new(state: State) -> View {
        match state {
            State::Missing => View {
                doc: Document::personal_only(),
                closed: None,
            },
            State::Loaded(doc) => View { doc, closed: None },
            State::Unreadable(why) => View {
                doc: Document::personal_only(),
                closed: Some(why),
            },
        }
    }

    pub fn organizations(&self) -> &[Organization] {
        &self.doc.organizations
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub fn is_closed(&self) -> bool {
        self.closed.is_some()
    }

    pub fn why_closed(&self) -> Option<&str> {
        self.closed.as_deref()
    }

    pub fn only_personal(&self) -> bool {
        self.doc.organizations.len() == 1
    }

    pub fn knows(&self, id: &str) -> bool {
        self.doc.find(id).is_some()
    }

    pub fn personal(&self) -> Resolved {
        Resolved {
            id: PERSONAL_ID.into(),
            source: Source::Default,
            known: true,
        }
    }

    pub fn resolve(&self, kind: Kind, key: &str, field: Option<&str>) -> Resolved {
        let field = field.map(str::trim).filter(|f| !f.is_empty());
        let shadow = if kind.is_account() {
            &self.doc.shadow.accounts
        } else {
            &self.doc.shadow.vault
        };
        let (id, source) = match field {
            Some(id) => (id.to_owned(), Source::Field),
            None => match shadow.get(&kind.shadow_key(key)) {
                Some(id) => (id.clone(), Source::Shadow),
                None => (PERSONAL_ID.to_owned(), Source::Default),
            },
        };
        let known = self.knows(&id);
        Resolved { id, source, known }
    }

    pub fn machine(&self, field: Option<&str>) -> Resolved {
        let field = field.map(str::trim).filter(|f| !f.is_empty());
        let id = field.unwrap_or(PERSONAL_ID).to_owned();
        let source = if field.is_some() {
            Source::Field
        } else {
            Source::Default
        };
        let known = self.knows(&id);
        Resolved { id, source, known }
    }

    pub fn gaps(&self, kind: Kind, listing: &[(String, Option<String>)]) -> Vec<Gap> {
        if self.only_personal() {
            return Vec::new();
        }
        listing
            .iter()
            .filter(|(key, field)| {
                self.resolve(kind, key, field.as_deref()).source == Source::Default
            })
            .map(|(key, _)| Gap {
                kind,
                key: key.clone(),
            })
            .collect()
    }

    pub fn folder(
        &self,
        folder: &Path,
        home: &Path,
        resolve: &dyn Fn(&Path) -> Option<PathBuf>,
    ) -> FolderMatch {
        folders::find(&self.doc.organizations, folder, home, resolve)
    }

    pub fn folder_here(&self, folder: &Path, home: &Path) -> FolderMatch {
        self.folder(folder, home, &folders::real)
    }
}
