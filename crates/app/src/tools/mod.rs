//! The app side of the other tools (Milestone 5, Task 6): Codex and Antigravity CLI sessions in
//! Chat and the notch, run by the daemon and switched off with it.
//!
//! The pure parts build and are tested on every system: [`requests`] (the `codex.*` and `agy.*`
//! ops, built the way `acting::Request` builds M3's), [`live`] (what the `tools` topic's events say
//! about each session: a turn running, a message waiting, a session the daemon holds), [`foreign`]
//! (Codex and Antigravity rows for the notch and Chat, and a conversation as the notch's lines),
//! [`agy_permission`] (an Antigravity permission as the daemon publishes it, its options in the
//! notch's order), [`codex_picker`] and [`agy_options`] (the model and options pickers),
//! [`second_opinion`] (the Second Opinion sheet's model) and [`renders`] (the `--render` views).
//! The GPUI views are [`view`], macOS only (`LINT-GPUI-MACOS-ONLY`).
//!
//! Nothing here starts a tool: every action goes through `actions::Actions::send`, which refuses
//! while acting isn't on, and every control is drawn from `Acting::control`, disabled with M3's off
//! message otherwise. The reads (`agy.conversation`) go through `Actions::read`, which needs no
//! acting.

pub mod agy_options;
pub mod agy_permission;
pub mod codex_picker;
pub mod foreign;
pub mod live;
pub mod renders;
pub mod requests;
pub mod second_opinion;
#[cfg(target_os = "macos")]
pub mod view;
