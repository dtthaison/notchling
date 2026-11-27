mod builder;
pub mod chunk;
pub mod corpus;
pub mod corpus_pages;
pub mod embed;
pub mod facets;
pub mod fts;
pub mod hybrid;
pub mod index;
mod meta;
pub mod models;
mod pages;
pub mod pipeline;
pub mod query;
pub mod range;
mod rank;
mod snippet;
pub mod store;
mod text;

pub use builder::{SessionTextBuilder, MAX_CHARS};
pub use meta::{age_text, codex_id, SessionMeta, SessionTool};
pub use pages::{artifact_id_in, find_pages, PageEntry};
pub use rank::{
    latest, lowercase, rank, words, IndexedSession, SessionHit, DEFAULT_LIMIT, RECENT_SECS,
};
pub use snippet::{collapse, highlights, snippet, DEFAULT_RADIUS};
pub use text::{
    claude_line, clean, codex_line, first_line_title, SessionRole, TranscriptLine, MAX_LINE_BYTES,
    MAX_MESSAGE_CHARS,
};

#[cfg(test)]
mod chunk_tests;
#[cfg(test)]
mod hybrid_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_corpus_pages;
#[cfg(test)]
mod tests_facets;
#[cfg(test)]
mod tests_fold;
#[cfg(test)]
mod tests_pages;
