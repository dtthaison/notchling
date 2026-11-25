use notchling_engine::search::query::fuse::{rrf, List, K};
use notchling_engine::settings::{KeySpec, KEYS};

const DOCS_MAX: usize = 200;
const DOC_CHARS: usize = 4000;

fn doc_of(spec: &KeySpec) -> (String, String) {
    let mut text = spec.shown_title().to_owned();
    for part in [spec.about, spec.doc] {
        if !part.is_empty() {
            text.push_str(". ");
            text.push_str(part);
        }
    }
    (spec.name.to_owned(), text.chars().take(DOC_CHARS).collect())
}

pub fn settings_docs() -> Vec<(String, String)> {
    KEYS.iter()
        .filter(|spec| spec.access.shown())
        .map(doc_of)
        .take(DOCS_MAX)
        .collect()
}

pub fn fuse_settings(
    text: &[String],
    semantic: &[(String, f64)],
    is_entry: impl Fn(&str) -> bool,
) -> Vec<String> {
    let meaning: Vec<String> = semantic
        .iter()
        .map(|(id, _)| id.clone())
        .filter(|id| is_entry(id))
        .collect();
    rrf(
        &[
            List {
                ids: text,
                weight: 1.0,
            },
            List {
                ids: &meaning,
                weight: 1.0,
            },
        ],
        K,
    )
    .into_iter()
    .map(|(id, _)| id)
    .collect()
}

#[cfg(test)]
#[path = "settings_semantic_tests.rs"]
mod tests;
