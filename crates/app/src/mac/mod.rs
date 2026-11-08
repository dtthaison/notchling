//! The macOS app over GPUI and AppKit: the menu bar item, the Dock presence, the main window, and
//! `--render`, which captures a view's window to a PNG (GPUI has no offscreen path; spike S4).

pub mod capture;
pub mod dock;
pub mod render;
pub mod shell;
pub mod tray;
pub mod views;
pub mod window;

use crate::cli::Command;

/// Runs a command; the exit code.
pub fn run(command: Command) -> i32 {
    match command {
        Command::Run => shell::run(),
        Command::Render { view, out } => render::run(&view, &out),
        Command::Version | Command::Help => 0,
    }
}
