//! E3a: the terminal. A screen emulator fed with what `claude attach` draws, the AskUserQuestion
//! reader on top of it, and a pty to run a program in.
//!
//! # Public API
//!
//! - [`TerminalScreen`]: `new(rows, cols)`, `feed(&[u8])`, `lines()`, `cells()`, `resize(rows, cols)`,
//!   `take_replies()` (cursor-position and status reports only: device-attribute queries are never
//!   answered), and the modes (`bracketed_paste()`, `alt_screen()`, `app_cursor_keys()`…).
//! - [`ask_dialog`]: [`ask_dialog::page`] reads the question page on screen, [`ask_dialog::shows`]
//!   checks which question it is, [`ask_dialog::is_review`] spots the review screen;
//!   [`dialog_open`] says whether any question or permission dialog is open.
//! - [`permission_dialog`]: [`permission_dialog::read`] reads a permission dialog ("Bash command",
//!   "Do you want to proceed?", "1. Yes"...), which option answers Allow, Always or Deny, and
//!   whether it asks about what a card showed ([`permission_dialog::PermissionDialog::asks`]).
//! - [`keys`]: what a key press or click sends.
//! - [`pty`]: [`Pty`] runs a program on a pseudo-terminal, started the way `forkpty` does (macOS and
//!   Linux; elsewhere [`PtyError::Unsupported`]); [`Terminal`] is a pty with a screen fed from it,
//!   which is what a reply is typed through. A missing folder or program fails the spawn
//!   ([`PtyError::Start`]); [`Terminal::write`] gives up with [`PtyError::Timeout`] on a program that
//!   has stopped reading.
//!
//! # Rules carried over from the Swift app
//!
//! - `forkpty`'s steps, not `posix_spawn`: started with posix_spawn, `claude attach` got SIGWINCH but
//!   never repainted for the new size. They are done one by one (`posix_openpt` with `O_CLOEXEC`,
//!   then `setsid`, `TIOCSCTTY` and `dup2` in the child) so no descriptor is inheritable by a process
//!   another thread starts meanwhile.
//! - Device-attribute queries (`CSI c`, `CSI > q`, `CSI > c`) go unanswered: answered, Claude Code
//!   dropped its colors and the answer landed in its input box.
//! - An inherited `NO_COLOR` is dropped from the child's environment.
//! - On resize, rows stay in place: Claude Code repaints only what changed.

pub mod ask_dialog;
mod csi;
pub mod keys;
mod live;
pub mod permission_dialog;
pub mod pty;
mod screen;
mod width;

pub use ask_dialog::dialog_open;
pub use live::Terminal;
pub use pty::{Pty, PtyCommand, PtyError, ReadOutcome, StartStep};
pub use screen::{TermCell, TermColor, TermStyle, TerminalScreen};
