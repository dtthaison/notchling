mod format;
#[rustfmt::skip]
mod generated;

pub use format::{format, lookup, shipped, Arg, Num, Text};
pub use generated::*;

#[cfg(test)]
#[path = "format_tests.rs"]
mod format_tests;
