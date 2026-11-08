//! The notch (part U2), read-only for Milestone 1: the black shape that hangs from the camera
//! housing, with the sessions' rows, a peek when one finishes or asks, and one session's last lines.
//! It never replies or answers: a question or permission shows with its options disabled.
//!
//! The pure parts build and are tested on every system: [`layout`] (the shape's size per state,
//! `NotchModel.size(for:)`, the canvas around the camera and the hit test that lets clicks outside
//! the shape through), [`model`] (rows, cards, hover and peeks), [`requests`] (the prompts the hook
//! holds, read-only), [`source`] (where it all is read from) and [`renders`] (the `--render`
//! views over a fixture). The GPUI view and the panel are
//! macOS only (`LINT-GPUI-MACOS-ONLY`).

#[cfg(target_os = "macos")]
pub mod agy_card;
pub mod answer;
#[cfg(target_os = "macos")]
pub mod answering;
#[cfg(target_os = "macos")]
pub mod backdrop;
#[cfg(target_os = "macos")]
pub mod cards;
pub mod layout;
pub mod model;
#[cfg(target_os = "macos")]
pub mod panel;
pub mod perch;
pub mod pet;
#[cfg(target_os = "macos")]
pub mod pet_view;
pub mod renders;
pub mod requests;
pub mod source;
#[cfg(target_os = "macos")]
pub mod view;
