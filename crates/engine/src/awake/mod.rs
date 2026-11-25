pub mod decide;
pub mod fake;
pub mod hold;
pub mod real;
pub mod state;

#[cfg(test)]
mod decide_tests;
#[cfg(test)]
mod hold_tests;

pub use decide::{busy, read, want, Kind, Mode, Setting, Work};
pub use fake::FakePower;
pub use hold::{Awake, Outcome, Power};
pub use real::RealPower;
pub use state::{holding_in, state_json};
