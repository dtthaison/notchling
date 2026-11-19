pub mod cache;
pub mod check;
pub mod picks;
pub mod profile;
pub mod renders;
pub mod setup;
pub mod wire;

#[cfg(target_os = "macos")]
mod check_view;
#[cfg(target_os = "macos")]
pub mod setup_view;
#[cfg(target_os = "macos")]
mod table_view;
#[cfg(target_os = "macos")]
pub mod view;

#[cfg(test)]
mod cache_tests;
#[cfg(test)]
mod check_tests;
#[cfg(test)]
mod picks_tests;
#[cfg(test)]
mod profile_tests;
#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod setup_tests;
#[cfg(test)]
mod wire_tests;
#[cfg(test)]
mod words_tests;
