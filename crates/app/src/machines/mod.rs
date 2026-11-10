//! Machines in the app (part E10, Milestone 6): the window of the `notchling-machines` binary
//! (`src/bin/machines.rs`), a GPUI window over the engine's `machines` module, apart from the
//! daemon. Machines | Jobs, a card per machine with its step rows (Check ✓/✗, Set up), the Fix
//! sheet that shows the exact commands before anything runs, the admin password for one step,
//! Set up all, the Running switch, Replace key…, the Update N machines and Rename banners, Add
//! machine, and the job cards with the morning items.
//!
//! Until handover the tool is **off**: without `--live` every process goes to the engine's
//! [`Refusing`](notchling_engine::machines::runner::Refusing) runner (no ssh, no `gh`), and without
//! `--vault` it has no secret values ([`NoSecrets`](notchling_engine::machines::step::NoSecrets)).
//! It reads the Swift app's `machines.json` and writes only its own documents in
//! `<data>/notchling.db` (the machines and the pending update offers, `machines/registry` and
//! `machines/pending-updates`; their old files move in once) and its audit log, merge worktrees,
//! scratch and cache under `<data>/daemon/machines/`
//! ([`MachineRoots`](notchling_engine::machines::roots::MachineRoots)). A render writes nothing
//! at all. A secret shows everywhere
//! as `[secret:<name>]`; the engine masks every text before the window sees it.
//!
//! The pure parts build and are tested on every system: [`cli`] (the binary's arguments), [`data`]
//! (where it reads and writes, and the engine's parts), [`pieces`] (the chip, the Running switch,
//! the log tail, the Fix sheet's words), [`forms`] (the Add and Job sheets' typed fields),
//! [`model`] (the window's state over the engine: every Fix starts in a method a button calls, and
//! only after its plan was shown) and [`renders`] (the `--render` views). The GPUI views are macOS
//! only (`LINT-GPUI-MACOS-ONLY`).

#[cfg(target_os = "macos")]
pub mod cards;
#[cfg(target_os = "macos")]
pub mod chrome_view;
pub mod cli;
pub mod data;
#[cfg(target_os = "macos")]
pub mod diff_view;
pub mod forms;
#[cfg(target_os = "macos")]
pub mod jobs_view;
pub mod model;
pub mod model_fixes;
pub mod model_sheet;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod model_tests_b;
#[cfg(test)]
mod model_tests_safety;
#[cfg(test)]
mod model_tests_sheet;
pub mod model_vault;
#[cfg(target_os = "macos")]
pub mod morning_view;
pub mod pieces;
#[cfg(test)]
mod pieces_tests;
pub mod renders;
pub mod runs;
#[cfg(target_os = "macos")]
pub mod sheets_view;
#[cfg(target_os = "macos")]
pub mod view;
#[cfg(target_os = "macos")]
pub mod window;
