use super::{Corpus, Doc, DocStamp, Tier};
use crate::pages::PageLibrary;
use crate::search::corpus_pages::{page_chunks_of, page_key};
use crate::search::PageEntry;
use std::io::Read;
use std::time::UNIX_EPOCH;

const MAX_PAGE_BYTES: u64 = 2_000_000;

pub struct PagesCorpus {
    library: PageLibrary,
}

impl PagesCorpus {
    pub fn new(library: PageLibrary) -> PagesCorpus {
        PagesCorpus { library }
    }

    fn key(entry: &PageEntry) -> String {
        page_key(entry)
    }

    fn modified(&self, entry: &PageEntry) -> Option<f64> {
        let path = self.library.local_path(entry)?;
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() {
            return None;
        }
        Some(
            meta.modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_secs_f64(),
        )
    }
}

impl Corpus for PagesCorpus {
    fn id(&self) -> &'static str {
        "pages"
    }

    fn tier(&self) -> Tier {
        Tier::Agent
    }

    fn scan(&self, window: Option<f64>, now: f64) -> Vec<DocStamp> {
        self.library
            .entries()
            .iter()
            .filter(|e| window.is_none_or(|w| now - e.updated < w))
            .filter_map(|e| {
                let modified = self.modified(e)?;
                Some(DocStamp {
                    key: Self::key(e),
                    stamp: format!("{}:{}", e.version, modified),
                    at: e.updated,
                })
            })
            .collect()
    }

    fn read(&self, doc: &DocStamp, _now: f64) -> Option<Doc> {
        let entry = self
            .library
            .entries()
            .into_iter()
            .find(|e| Self::key(e) == doc.key)?;
        let path = self.library.local_path(&entry)?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(MAX_PAGE_BYTES)
            .read_to_end(&mut bytes)
            .ok()?;
        let html = String::from_utf8_lossy(&bytes);
        let mut facets = Vec::new();
        if !entry.account.is_empty() {
            facets.push(("owner".to_string(), entry.account.clone()));
        }
        if !entry.project.is_empty() {
            facets.push(("project".to_string(), entry.project.clone()));
        }
        Some(Doc {
            key: doc.key.clone(),
            title: entry.title.clone(),
            at: entry.updated,
            facets,
            chunks: page_chunks_of(&html),
        })
    }
}
