mod pages;
mod sessions;
mod settings;

use super::chunk::ChunkText;

pub use pages::PagesCorpus;
pub use sessions::SessionsCorpus;
pub use settings::SettingsCorpus;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Agent,
    App,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DocStamp {
    pub key: String,
    pub stamp: String,
    pub at: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub key: String,
    pub title: String,
    pub at: f64,
    pub facets: Vec<(String, String)>,
    pub chunks: Vec<ChunkText>,
}

pub trait Corpus: Send + Sync {
    fn id(&self) -> &'static str;
    fn tier(&self) -> Tier;
    fn scan(&self, window: Option<f64>, now: f64) -> Vec<DocStamp>;
    fn read(&self, doc: &DocStamp, now: f64) -> Option<Doc>;
}

#[cfg(test)]
mod corpus_tests;
