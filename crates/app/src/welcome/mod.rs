//! First run, About, updates and the handover checklist (part U11, Milestone 6).
//!
//! The app doesn't own the hook, the vault, logins, the phone bridge or sessions yet: the Swift app
//! does, until the maintainer hands each area over (decision 0011). This part says so plainly,
//! brings over the Swift app's settings, and gives the maintainer one place to hand areas over.
//!
//! - [`first_run`]: what the first-run view shows, over [`facts`] (is the Swift app here, what
//!   `install.plan` says about Claude Code's hooks) and [`hooks`] (the two install ops).
//! - [`defaults`]: the Swift app's settings, read-only through a trait, into the app's own
//!   `settings.json`; [`xml`] is the small reader it uses for the property list.
//! - [`about`] and [`updates`]: version, build, license, links; the update check against the
//!   Phase 6 feed, behind its kill switch, which never installs anything.
//! - [`handover`] and [`switches`]: the checklist, and the daemon's `accounts`, `vault` and
//!   `install` switches with the user's choice kept in [`own`].
//! - [`renders`]: `--render first-run | about | handover`.
//!
//! The pure parts build and are tested on every system; the GPUI views (`view`, `window`) are
//! macOS only (`LINT-GPUI-MACOS-ONLY`).

pub mod about;
pub mod defaults;
pub mod facts;
pub mod first_run;
pub mod handover;
#[cfg(target_os = "macos")]
pub mod handover_view;
pub mod hooks;
pub mod own;
pub mod renders;
pub mod switches;
pub mod updates;
#[cfg(target_os = "macos")]
pub mod view;
#[cfg(target_os = "macos")]
pub mod window;
pub mod xml;
