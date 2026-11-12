//! The macOS app over GPUI and AppKit: the menu bar item, the Dock presence, the main window, and
//! `--render`, which captures a view's window to a PNG (GPUI has no offscreen path; spike S4).

pub mod app_icons;
pub mod bench;
pub mod bench_events;
pub mod bench_scroll;
pub mod capture;
pub mod dock;
pub mod gate;
pub mod hotkey;
pub mod native_spin;
pub mod native_spin_geometry;
pub mod pair_prompt;
pub mod render;
pub mod restart_notice;
pub mod shell;
pub mod tray;
pub mod vault_prompts;
pub mod views;
pub mod window;

use crate::cli::Command;

/// Runs a command; the exit code.
pub fn run(command: Command) -> i32 {
    match command {
        Command::Run {
            data_root,
            open_chat,
        } => shell::run(data_root, open_chat),
        Command::Render {
            view,
            out,
            data_root,
        } => render::run(&view, &out, data_root.as_deref()),
        Command::Version | Command::Help => 0,
    }
}
