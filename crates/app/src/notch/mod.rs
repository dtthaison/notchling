//! The notch (part U2), read-only for Milestone 1: the black shape that hangs from the camera
//! housing, with the sessions' rows, a peek when one finishes or asks, and one session's last lines.
//! It never replies or answers: a question or permission shows with its options disabled.
//!
//! [`layout`] is the pure geometry: the shape's size per state (`NotchModel.size(for:)`), the canvas
//! around the camera and the hit test that lets clicks outside the shape through.

pub mod layout;
