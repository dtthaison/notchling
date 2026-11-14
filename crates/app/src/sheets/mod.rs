pub mod agents;
#[cfg(test)]
mod agents_tests;
#[cfg(target_os = "macos")]
pub mod agents_view;
pub mod banner;
pub mod capabilities;
#[cfg(test)]
mod capabilities_tests;
#[cfg(target_os = "macos")]
pub mod capabilities_view;
pub mod client_check;
pub mod client_model;
#[cfg(test)]
mod client_model_check_tests;
#[cfg(test)]
mod client_model_tests;
#[cfg(target_os = "macos")]
pub mod client_view;
pub mod hook_rows;
#[cfg(test)]
mod hook_rows_tests;
pub mod new_session;
pub mod renders;
#[cfg(target_os = "macos")]
pub mod second_opinion_view;
pub mod settings;
#[cfg(target_os = "macos")]
pub mod settings_hooks_view;
#[cfg(target_os = "macos")]
pub mod settings_view;
#[cfg(target_os = "macos")]
pub mod view;
