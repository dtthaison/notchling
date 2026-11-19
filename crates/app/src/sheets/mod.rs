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
pub mod models_refresh;
pub mod models_rows;
#[cfg(test)]
mod models_rows_tests;
pub mod new_session;
#[cfg(test)]
mod new_session_chrome_tests;
#[cfg(test)]
mod new_session_run_on_tests;
pub mod renders;
pub mod run_on;
#[cfg(target_os = "macos")]
pub mod run_on_view;
#[cfg(target_os = "macos")]
pub mod second_opinion_view;
pub mod settings;
pub mod settings_cluster;
#[cfg(test)]
mod settings_cluster_tests;
#[cfg(target_os = "macos")]
pub mod settings_hooks_view;
#[cfg(target_os = "macos")]
pub mod settings_skills_view;
#[cfg(test)]
mod settings_tabs_tests;
#[cfg(target_os = "macos")]
pub mod settings_view;
#[cfg(test)]
mod settings_words_tests;
pub mod sidecar_rows;
pub mod skills_models;
#[cfg(test)]
mod skills_models_tests;
pub mod skills_rows;
#[cfg(test)]
mod skills_rows_tests;
#[cfg(target_os = "macos")]
pub mod view;
