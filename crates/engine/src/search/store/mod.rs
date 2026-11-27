mod format;
mod set;
mod vectors;

pub use format::VecFormat;
pub use set::{DocHit, Keep, SetDoc, Sets, VectorSet};
pub use vectors::{Coverage, Stats, VectorDb};

#[cfg(test)]
mod format_tests;
#[cfg(test)]
mod set_tests;
#[cfg(test)]
mod vectors_tests;
