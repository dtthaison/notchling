mod builder;
pub mod fts;
pub mod index;
mod meta;
mod pages;
mod rank;
mod snippet;
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
mod tests;
#[cfg(test)]
mod tests_pages;
