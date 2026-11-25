pub mod category;
pub mod filter;
mod fold;
mod fold_table;
pub mod fuse;
pub mod parse;
pub mod shape;
pub mod when;

pub use fold::{fold, fold_char, words};

#[cfg(test)]
mod category_tests;
#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod fold_tests;
#[cfg(test)]
mod fuse_tests;
#[cfg(test)]
mod parse_facets_tests;
#[cfg(test)]
mod shape_tests;
#[cfg(test)]
mod when_tests;
