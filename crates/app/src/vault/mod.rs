//! The Secret Vault in the app (part U9, Milestone 6): the vault sheet (the list without values,
//! add, edit, remove, how-to, expiry, Renew), the approval prompt an agent's `vault.use` waits on
//! (Deny / Allow Once / Always Allow, which only the app can answer) and the form an agent's
//! `vault.request` opens. Everything goes through the daemon's app-tier ops ([`wire`]) and is
//! disabled, with the daemon's own off message, while the `vault` switch is off. No value is ever
//! shown after it is saved. The view models are pure and build on every system; the GPUI views
//! are macOS only (`LINT-GPUI-MACOS-ONLY`).

pub mod approval;
pub mod form;
pub mod list;
pub mod renders;
pub mod store;
pub mod wire;
