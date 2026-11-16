pub mod feed;
pub mod install;
pub mod model;
pub mod move_sheet;
#[cfg(target_os = "macos")]
mod move_view;
pub mod promote;
pub mod promote_sheet;
#[cfg(target_os = "macos")]
mod promote_view;
pub mod renders;
pub mod replication;
pub mod secrets;
#[cfg(target_os = "macos")]
pub mod secrets_view;
#[cfg(target_os = "macos")]
pub mod view;
#[cfg(target_os = "macos")]
mod view_rows;
#[cfg(target_os = "macos")]
mod view_secrets;
pub mod wire;

pub const WIDTH: f32 = 680.0;
pub const HEIGHT: f32 = 560.0;

#[cfg(test)]
mod feed_tests;
#[cfg(test)]
mod install_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod move_sheet_tests;
#[cfg(test)]
mod promote_tests;
#[cfg(test)]
mod replication_tests;
#[cfg(test)]
mod secrets_tests;
