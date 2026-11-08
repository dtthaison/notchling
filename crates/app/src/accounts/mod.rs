//! Accounts, lanes, the other providers, limits and cost in the app (part U5, Milestone 6): the
//! Accounts window and Chat's Accounts sheet, the header's account menu and limit meters, the
//! Token Cost page. Everything goes through the daemon's ops ([`wire`]) and is disabled, with the
//! daemon's own off message, while the `accounts` switch is off ([`crate::switches`]); the lists
//! still show. The view models are pure and build on every system; the GPUI views are macOS only
//! (`LINT-GPUI-MACOS-ONLY`).

pub mod cost;
pub mod flows;
pub mod limits;
pub mod menu;
pub mod model;
pub mod renders;
pub mod store;
#[cfg(target_os = "macos")]
pub mod view;
#[cfg(target_os = "macos")]
pub mod view_cost;
#[cfg(target_os = "macos")]
pub mod view_rows;
#[cfg(target_os = "macos")]
pub mod view_tabs;
#[cfg(target_os = "macos")]
pub mod window;
pub mod wire;
