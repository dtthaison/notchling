//! The Phone panel (part U6, Milestone 4): a popover from the Phone button at the top right of
//! Chat's header with the bridge switch, the pairing QR and one-time code, the paired phones with
//! Revoke, and the tunnel's status. The daemon owns the bridge (decision 0011): this side only
//! asks, through the `bridge.*` ops ([`wire`]), and keeps what it was told ([`bridge`]).
//!
//! The pure parts build and are tested on every system: [`wire`] (op shapes and readers), [`qr`]
//! (the code as modules), [`bridge`] (the daemon's state and the user's `bridgeOn` choice) and
//! [`panel`] (what is drawn, each control with the reason it is disabled). [`renders`] is
//! `--render phone-panel`. The GPUI view is `view`, macOS only (`LINT-GPUI-MACOS-ONLY`).
//!
//! A phone that gave the right code waits for the person. The daemon asks only an app that follows
//! its `bridge` topic, so [`pairing`] is that follower; [`approval`] is the pure Allow/Deny prompt
//! and its queue; `prompt_view` (opened by `mac::pair_prompt`) draws it in a floating window
//! whether or not the panel is open (`--render phone-pair-request`).

pub mod approval;
pub mod bridge;
pub mod pairing;
pub mod panel;
#[cfg(target_os = "macos")]
pub mod prompt_view;
pub mod qr;
pub mod renders;
#[cfg(target_os = "macos")]
pub mod view;
pub mod wire;
