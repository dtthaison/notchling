mod model;
mod store;

pub use model::{
    new_id, Change, InsertError, Kind, New, Row, State, Tool, Via, Why, MAX_ROWS, MAX_TEXT,
};
pub use store::OutboxStore;

#[cfg(test)]
#[path = "model_tests.rs"]
mod model_tests;
#[cfg(test)]
#[path = "store_tests.rs"]
mod store_tests;
