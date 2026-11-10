//! First run, About and updates (part U11, Milestone 6).
//!
//! This part brings over the Swift app's settings and says what the first run finds. The daemon's
//! switches (acting, the bridge, logins, the vault, install) are in Settings
//! (`sheets::settings`).
//!
//! - [`first_run`]: what the first-run view shows, over [`facts`] (is the Swift app here, what
//!   `install.plan` says about Claude Code's hooks) and [`hooks`] (the two install ops).
//! - [`defaults`]: the Swift app's settings, read-only through a trait, into the app's own
//!   `settings.json`; [`xml`] is the small reader it uses for the property list.
//! - [`about`] and [`updates`]: version, build, license, links; the update check against the
//!   Phase 6 feed, behind its kill switch, which never installs anything.
//! - [`own`]: the first-run flag and the user's switch choices in the app's settings.
//! - [`renders`]: `--render first-run | about`.
//!
//! The pure parts build and are tested on every system; the GPUI views (`view`, `window`) are
//! macOS only (`LINT-GPUI-MACOS-ONLY`).

pub mod about;
pub mod defaults;
pub mod facts;
pub mod first_run;
pub mod hooks;
pub mod own;
pub mod renders;
pub mod updates;
#[cfg(target_os = "macos")]
pub mod view;
#[cfg(target_os = "macos")]
pub mod window;
pub mod xml;
