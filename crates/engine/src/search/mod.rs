//! E1: session search, the pure half: what a transcript line contributes to the index, how a
//! query matches and ranks sessions, and the snippet shown around a match. Ported from the Swift
//! app's `SessionSearch.swift` and `PageLibrary.find`. The index that reads the files and caches
//! them is [`index`] (E7); the panel is the app's.
//!
//! # Public API
//!
//! Extraction ([`text`]):
//! - [`claude_line`] and [`codex_line`] turn one transcript line (bytes) into a
//!   [`TranscriptLine`]: a message with its [`SessionRole`] and optional working folder, or a
//!   Claude Code custom or AI title. Lines over [`MAX_LINE_BYTES`], meta lines, sidechains, tool
//!   output, thinking and injected context give `None`. Messages are trimmed and capped at
//!   [`MAX_MESSAGE_CHARS`] by [`clean`].
//! - [`SessionTextBuilder`] gathers lines into a conversation's `title()`, `cwd()`,
//!   `first_prompt()` and `text()` (newest [`MAX_CHARS`] characters kept).
//!
//! Matching ([`rank`] module):
//! - [`words`] splits and lowercases a query; [`IndexedSession::new`] lowercases a session's
//!   title, folder and text once; [`rank`] keeps sessions where every word is in the title, folder
//!   or text, scores them (title, folder, occurrences, phrase, recency) and returns the best
//!   `limit` as [`SessionHit`]s; [`latest`] lists the newest with no snippet.
//! - [`snippet()`], [`collapse`] and [`highlights`] build the snippet and mark the words in it
//!   (byte ranges into the snippet).
//! - Times are seconds since 1970 as `f64`; [`age_text`] gives "now", "5m", "3h", "2d", "3w".
//!
//! Records and pages:
//! - [`SessionMeta`] and [`SessionTool`] are the index entry, with the Swift cache's JSON keys;
//!   [`codex_id`] reads a session id from a rollout file name.
//! - [`PageEntry`], [`artifact_id_in`] and [`find_pages`] find saved Artifact pages by link, id
//!   or words.
//!
//! # Unicode rulings
//!
//! The Swift code calls `lowercased()` and searches bytes; it never calls `localizedStandardContains`
//! or `folding(options:)`. The port keeps that, and decides the edge cases this way:
//! - Case folding is per Unicode scalar with no context rules, so `Đ` matches `đ` and a final `Σ`
//!   becomes `σ`, as in Swift. Rust's `str::to_lowercase` (final sigma) is not used.
//! - There is no diacritic folding: `sua` does not find `sửa`.
//! - There is no normalization: a decomposed `e` + U+0301 does not find a precomposed `é`. Swift
//!   treats the two as equal for a title or folder match only (Foundation's `contains`); the body
//!   is bytewise there too.
//! - Lengths and caps (message, text and title lengths) count Unicode scalars, not grapheme
//!   clusters, which would need a segmentation crate.
//! - Words are separated by Unicode White_Space (`char::is_whitespace`); page lookup separates
//!   words by the space character only, like Swift.
//! - An empty word is ignored; when only empty words remain, nothing matches.
//! - A snippet is cut from the original text when lowercasing kept its byte length, else from the
//!   lowercased text; [`highlights`] returns nothing if lowercasing changed the snippet's length.

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
