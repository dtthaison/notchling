pub mod feed;
pub mod model;
pub mod renders;
#[cfg(target_os = "macos")]
pub mod view;
pub mod wire;

pub const WIDTH: f32 = 680.0;
pub const HEIGHT: f32 = 560.0;

#[cfg(test)]
mod feed_tests;
#[cfg(test)]
mod model_tests;
