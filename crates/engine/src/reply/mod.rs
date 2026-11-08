//! E3b: replies to a background session, as the Swift app's `ReplySender` types them.
//!
//! Everything here is pure except [`exec`], which plays a [`plan::Plan`] into anything that is a
//! [`exec::Pty`]. The planners say which keys go in, in which order, with which pauses and which
//! screen checks; the timings are data ([`keys`]), pinned to the Swift constants by tests.
//!
//! | Part | What it is |
//! | --- | --- |
//! | [`keys`] | The key bytes, the bracketed paste with its escapes taken out, the pauses, the dialog markers |
//! | [`question`] | Answers to an AskUserQuestion: digits, Tab, the text row, the review `1`; the hook's answers |
//! | [`plan`] | The planners: reply, answer, take back the queue, interrupt |
//! | [`exec`] | The executor and its `Pty` and `ScreenCheck` traits |
//! | [`shell`] | `!` commands copied from a terminal, put back together before they're sent |
//! | [`popall`] | Reading `popAll` back from the transcript, which is how a take-back is known to have worked |
//!
//! Nothing here starts `claude` or opens a pty: the daemon hands the executor a pty from E3a.

pub mod exec;
pub mod keys;
pub mod plan;
pub mod popall;
pub mod question;
pub mod shell;

#[cfg(test)]
mod exec_tests;
#[cfg(test)]
mod pin_tests;
