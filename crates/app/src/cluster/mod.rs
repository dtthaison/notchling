pub mod add;
pub mod add_job;
pub mod add_parts;
pub mod add_plan;
#[cfg(target_os = "macos")]
mod add_plan_view;
mod add_register;
pub mod add_runs;
pub mod add_sheet;
pub mod add_source;
#[cfg(target_os = "macos")]
mod add_view;
pub mod feed;
pub mod install;
pub mod machine_add;
pub mod model;
pub mod move_sheet;
#[cfg(target_os = "macos")]
mod move_view;
pub mod not_joined;
#[cfg(target_os = "macos")]
mod not_joined_view;
pub mod promote;
pub mod promote_sheet;
#[cfg(target_os = "macos")]
mod promote_view;
pub mod renders;
pub mod replication;
pub mod secrets;
#[cfg(target_os = "macos")]
pub mod secrets_view;
pub mod tool_runs;
pub mod tool_setup;
#[cfg(target_os = "macos")]
mod tool_setup_view;
pub mod tool_updates;
#[cfg(target_os = "macos")]
mod tool_updates_view;
pub mod updates;
mod updates_state;
#[cfg(target_os = "macos")]
mod updates_view;
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
mod add_rig_tests;
#[cfg(test)]
mod add_sheet_tests;
#[cfg(test)]
mod add_tests;
#[cfg(test)]
mod feed_tests;
#[cfg(test)]
mod install_tests;
#[cfg(test)]
mod login_own_tests;
#[cfg(test)]
mod machine_add_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod move_sheet_tests;
#[cfg(test)]
mod not_joined_tests;
#[cfg(test)]
mod promote_tests;
#[cfg(test)]
mod replication_tests;
#[cfg(test)]
mod secrets_tests;
#[cfg(test)]
mod tool_setup_tests;
#[cfg(test)]
mod tool_updates_tests;
#[cfg(test)]
mod updates_state_tests;
#[cfg(test)]
mod updates_tests;
