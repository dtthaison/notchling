//! The Chat window, read-only (part U1, Milestone 1): a sidebar of sessions, the conversation, a
//! header and an inspector, over the engine's readers. It only reads files. Replies, actions and
//! the reply box arrive with Milestone 3.
//!
//! The view models here are pure and tested on every system: [`data`] says where to read (the
//! real folders, or a fixture root so `--render chat` never touches `~/.claude`), [`sidebar`],
//! [`rows`], [`header`] and [`inspector`] turn the engine's types into what the views draw,
//! [`activity`] words the live line above the reply box, [`follow`] decides when the conversation
//! follows its newest message, [`steps`] folds a run of tool calls into one chip, [`markdown`] reads a reply's Markdown, [`page_panel`] decides what a clicked link opens, [`file_panel`] reads a clicked file for the panel, [`paths`] links the paths a message names, and
//! [`format`] holds the small text rules (`ChatFormat` in the Swift app). The GPUI views are in
//! [`view`], macOS only for now.

pub mod activity;
pub mod bench_fixture;
pub mod claude_picker;
pub mod closed;
pub mod codex_list;
pub mod composer;
pub mod data;
pub mod drafts;
pub mod drops;
pub mod escape;
pub mod file_panel;
pub mod follow;
pub mod format;
pub mod header;
pub mod image_size;
pub mod inspector;
pub mod lane_limits;
pub mod limit_ring;
pub mod loader;
pub mod log;
#[cfg(test)]
mod log_tests;
pub mod markdown;
pub mod markdown_cache;
pub mod message_meta;
pub mod outgoing;
pub mod page_panel;
#[cfg(test)]
mod parse_cost_tests;
pub mod paths;
pub mod pickers;
pub mod prompts;
pub mod reload;
pub mod renders;
pub mod rows;
pub mod selection;
pub mod sidebar;
pub mod simulator;
pub mod slash_menu;
pub mod steps;
pub mod subagent_view;
pub mod tasks_bar;
pub mod terminal;
pub mod used;
#[cfg(target_os = "macos")]
pub mod view;
