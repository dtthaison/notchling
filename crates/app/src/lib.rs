#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod accounts;
pub mod acting;
pub mod actions;
pub mod activity;
pub mod appearance;
pub mod chat;
pub mod cli;
pub mod crash_log;
pub mod dock;
pub mod inbox;
pub mod language;
pub mod keep_awake;
#[cfg(target_os = "macos")]
pub mod mac;
pub mod machines;
pub mod menu;
pub mod notch;
pub mod pages;
pub mod phone;
pub mod search;
pub mod session_source;
pub mod sheets;
pub mod switches;
#[cfg(test)]
mod testutil;
pub mod tools;
pub mod transcripts;
pub mod vault;
pub mod views;
pub mod welcome;
pub mod window_frame;

pub const NOT_SUPPORTED: &str = "notchling-app: not supported yet on this system";

pub fn crate_name() -> &'static str {
    "app"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_itself() {
        assert_eq!(crate_name(), "app");
    }
}
