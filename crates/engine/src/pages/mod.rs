//! E7: the Artifact pages library. A local copy of every page Claude Code publishes as an
//! Artifact, readable whatever account is active (artifacts are private to the login that
//! published them, and the source file often sits in a job's tmp folder that goes with the job).
//! Ported from the Swift app's `PageLibrary`, `OwnerPublish` and the Artifact branches of
//! `HookBridge`.
//!
//! # On disk (the Swift app's layout, so both can read one folder)
//!
//! ```text
//! <data dir>/pages/pages.json            {"pages": [{folder, file, title, url, source, project,
//!                                          account, updated, version}, ...]}, under flock(.lock)
//! <data dir>/pages/<folder>/index.html   the page, wrapped in a doctype skeleton (or index.md)
//! <data dir>/pages/<folder>/...          supporting files at their published paths
//! <data dir>/pages/index.html            a plain list of everything
//! ```
//!
//! # Rules
//!
//! - A library is opened [`PageLibrary::read_only`] or [`PageLibrary::writable`]. The daemon opens
//!   the Swift app's folder read-only (decision 0011: the Swift app owns the page hook until E13);
//!   only a test folder is writable there.
//! - `pages.json` is only written holding an exclusive `flock` on `.lock`, so two publishes landing
//!   at once keep both entries.
//! - The first publisher is the page's `account` and stays it on republish, since only that account
//!   can republish to the link. Each publish keeps its `version`.
//! - A folder or file name read from `pages.json` is data: [`PageLibrary::local_path`] gives `None`
//!   for one that is not a plain name, and published supporting paths with `..` or a leading `/`
//!   are not copied. A page id never becomes a path at all: ids only select entries.
//!
//! # Public API
//!
//! - [`PageLibrary`]: [`entries`](PageLibrary::entries), [`find`](PageLibrary::find) (link, id or
//!   words, via [`crate::search::find_pages`]), [`describe`](PageLibrary::describe),
//!   [`local_path`](PageLibrary::local_path), [`record`](PageLibrary::record) (a `PostToolUse`
//!   event), [`remove`](PageLibrary::remove).
//! - [`hook`]: the Artifact hook events as pure functions ([`hook::pre_tool_use`],
//!   [`hook::post_tool_use`]). Not wired into `notchling hook` until E13.
//! - [`owner`]: publishing as a page's owner, over an [`owner::HttpClient`] and an
//!   [`owner::TokenSource`]; base58 link id to UUID ([`owner::uuid_from_short_id`]).
//! - [`content`]: titles, the wrapper, folder names, facts in a tool result.

pub mod clock;
pub mod content;
pub mod hook;
mod library;
mod listpage;
mod lock;
pub mod owner;
mod record;

pub use library::{find_in, safe_component, safe_relative, PageLibrary, PagesError};
pub use listpage::html as list_page_html;
pub use record::{
    flatten, join_normalized, read_bounded, RecordContext, MAX_FILE_BYTES, MAX_TOTAL_BYTES,
};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_hook;
#[cfg(test)]
mod tests_owner;
