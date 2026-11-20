pub mod decide;
pub mod event;
pub mod policy;
pub mod rules;
pub mod shell;
pub mod spool;
pub mod state;

pub use decide::{decide, record, Env};
pub use event::{parse, Event, Kind};
pub use rules::{Action, Rule, Would};

pub const DIR: &str = "guard";

#[cfg(test)]
mod decide_tests;
#[cfg(test)]
mod rules_tests;
#[cfg(test)]
mod spool_tests;
#[cfg(test)]
mod state_tests;
