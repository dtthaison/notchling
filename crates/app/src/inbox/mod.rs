pub mod feed;
pub mod model;
pub mod renders;
pub mod system;
#[cfg(target_os = "macos")]
pub mod view;

pub const WIDTH: f32 = 360.0;
pub const MAX_HEIGHT: f32 = 520.0;

#[cfg(test)]
mod versions_rows_tests;
