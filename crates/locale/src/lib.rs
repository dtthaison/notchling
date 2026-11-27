mod choose;
pub mod fmt;
mod format;
#[rustfmt::skip]
mod generated;
mod numbers;
mod pseudo;
mod state;
#[doc(hidden)]
pub mod testing;

pub use choose::{
    active, apply, choose, start, Choice, Inputs, LOCALE_ENV, MARK_FALLBACK_ENV, PSEUDO,
};
pub use format::{format, lookup, shipped, Arg, Num, Text};
pub use generated::*;

#[cfg(test)]
extern crate self as notchling_locale;

#[cfg(test)]
#[path = "choose_tests.rs"]
mod choose_tests;

#[cfg(test)]
#[path = "conformance_tests.rs"]
mod conformance_tests;

#[cfg(test)]
#[path = "fmt_tests.rs"]
mod fmt_tests;

#[cfg(test)]
#[path = "format_tests.rs"]
mod format_tests;

#[cfg(test)]
#[path = "pseudo_tests.rs"]
mod pseudo_tests;
