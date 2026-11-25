use super::{Corpus, Doc, DocStamp, Tier};
use crate::search::chunk::{hash, ChunkKind, ChunkText};
use std::sync::Mutex;

#[derive(Default)]
pub struct SettingsCorpus {
    docs: Mutex<Vec<(String, String)>>,
}

impl SettingsCorpus {
    pub fn new() -> SettingsCorpus {
        SettingsCorpus::default()
    }

    pub fn set(&self, docs: Vec<(String, String)>) {
        *self.docs.lock().unwrap_or_else(|e| e.into_inner()) = docs;
    }
}

fn stamp(text: &str) -> String {
    hash(text).iter().map(|b| format!("{b:02x}")).collect()
}

impl Corpus for SettingsCorpus {
    fn id(&self) -> &'static str {
        "settings"
    }

    fn tier(&self) -> Tier {
        Tier::App
    }

    fn scan(&self, _window: Option<f64>, _now: f64) -> Vec<DocStamp> {
        self.docs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(key, text)| DocStamp {
                key: key.clone(),
                stamp: stamp(text),
                at: 0.0,
            })
            .collect()
    }

    fn read(&self, doc: &DocStamp, _now: f64) -> Option<Doc> {
        let docs = self.docs.lock().unwrap_or_else(|e| e.into_inner());
        let (key, text) = docs.iter().find(|(key, _)| *key == doc.key)?;
        let mut facets = Vec::new();
        if let Some((section, _)) = key.split_once('.') {
            facets.push(("section".to_string(), section.to_string()));
        }
        Some(Doc {
            key: key.clone(),
            title: key.clone(),
            at: 0.0,
            facets,
            chunks: vec![ChunkText {
                kind: ChunkKind::Entry,
                text: text.clone(),
            }],
        })
    }
}
