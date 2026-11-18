use std::path::{Path, PathBuf};

use notchling_engine::contract::channel::{Channel, Opener};
use notchling_engine::contract::channel_draft;
use notchling_engine::contract::channel_remote;
use notchling_engine::contract::channels::{APP, REMOTE};
use notchling_locale as t;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Words {
    pub placeholder: &'static str,
    pub tip: &'static str,
}

pub fn words(channel: &dyn Channel) -> Option<Words> {
    match channel.id() {
        APP => Some(Words {
            placeholder: t::chat::antigravity_draft_placeholder(),
            tip: t::chat::antigravity_send_tip(),
        }),
        REMOTE => Some(Words {
            placeholder: t::chat::antigravity_remote_placeholder(),
            tip: t::chat::antigravity_remote_send_tip(),
        }),
        _ => None,
    }
}

pub fn in_panel(remote_in: &str) -> bool {
    remote_in != "browser"
}

pub fn remote_root(antigravity_dir: &Path) -> PathBuf {
    antigravity_dir
        .parent()
        .unwrap_or(antigravity_dir)
        .join("antigravity")
}

pub struct Target<'a> {
    pub in_panel: bool,
    pub remote_root: &'a Path,
    pub email: Option<&'a str>,
    pub row_id: &'a str,
}

pub fn hand_over(
    channel: &dyn Channel,
    opener: &mut dyn Opener,
    target: &Target,
    text: &str,
) -> Option<Result<&'static str, &'static str>> {
    match channel.id() {
        APP => Some(
            channel_draft::hand_over(opener, text)
                .map(|()| t::chat::antigravity_draft_note())
                .map_err(|_| t::chat::antigravity_missing()),
        ),
        REMOTE => Some(
            channel_remote::hand_over(
                opener,
                target.remote_root,
                target.email,
                target.row_id,
                text,
                target.in_panel,
            )
            .map(|()| t::chat::antigravity_remote_note())
            .map_err(|_| t::chat::antigravity_remote_missing()),
        ),
        _ => None,
    }
}

#[cfg(test)]
#[path = "agy_hand_off_tests.rs"]
mod tests;
