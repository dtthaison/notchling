//! The Chat window, read-only (part U1, Milestone 1): a sidebar of sessions, the conversation, a
//! header and an inspector, over the engine's readers. It only reads files. Replies, actions and
//! the reply box arrive with Milestone 3.
//!
//! The view models here are pure and tested on every system: [`data`] says where to read (the
//! real folders, or a fixture root so `--render chat` never touches `~/.claude`), [`sidebar`],
//! [`rows`], [`header`] and [`inspector`] turn the engine's types into what the views draw, and
//! [`format`] holds the small text rules (`ChatFormat` in the Swift app). The GPUI views are in
//! [`view`], macOS only for now.

pub mod codex_list;
pub mod data;
pub mod format;
pub mod header;
pub mod inspector;
pub mod reload;
pub mod rows;
pub mod sidebar;
#[cfg(target_os = "macos")]
pub mod view;
