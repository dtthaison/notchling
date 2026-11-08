//! E14: the settings store, migration from the Swift app's keys, data folders per OS.
//!
//! # Public API
//!
//! - [`Settings::open`] / [`Settings::get`] / [`Settings::set`]: a JSON file (`settings.json` in
//!   [`data_dir`]), `{"version": 1, "values": {...}}`, written atomically (temp file, then rename) with
//!   mode 0600. A missing file is an empty store; an unreadable one is [`SettingsError::Corrupt`] and is
//!   never written over. Reads fall back to the key's default; `set` refuses unknown keys and values
//!   of the wrong type.
//! - [`KEYS`]: the table below, as data.
//! - [`migrate`]: the Swift app's exported UserDefaults in, new settings out. The app exports the
//!   `local.notchling` domain (and `local.awake.menubar` when upgrading from Awake) as JSON; reading
//!   the real domain is the app's job.
//! - [`import`]: [`migrate`] into a [`Settings`] and save; values the new app already has win.
//! - [`data_dir`] / [`DirEnv`]: the data folder as a pure function of the environment.
//!
//! # The keys
//!
//! 36 keys, enumerated from `legacy/macos/Sources` (the spec's "30" was an estimate). "Awake" marks
//! the 24 that `AppDelegate.migrateFromAwake` copies from the old `local.awake.menubar` domain, which
//! [`migrate`] copies the same way (only when the current domain lacks the key).
//!
//! | Key | Type | Default | Awake | Meaning |
//! | --- | --- | --- | --- | --- |
//! | `mode` | `off`/`on`/`whileWorking` | `off` | yes | Keep Mac Awake |
//! | `active` | bool | false | yes | Awake v1's flag; folded into `mode` by the migration |
//! | `keepDisplayOn` | bool | false | yes | Keep the display on too |
//! | `continueAfterSleep` | bool | true | yes | Resume sessions after wake |
//! | `hideNotch` | bool | false | yes | Hide the notch |
//! | `answerFromNotch` | bool | true | yes | Answer prompts from the notch |
//! | `keepPages` | bool | true | yes | Keep local copies of artifacts |
//! | `showPet` | bool | true | yes | The notch pet |
//! | `replyFromNotch` | bool | true | yes | Reply box in the notch |
//! | `showLastReply` | bool | true | yes | Last reply in the notch |
//! | `askedAccessibility` | bool | false | yes | Accessibility prompt shown once |
//! | `notchWidth` | `small`/`medium`/`large` | `medium` | yes | Notch panel width |
//! | `dismissed` | dict string to number | `{}` | yes | Done rows cleared |
//! | `hiddenAgents` | dict string to number | `{}` | yes | Hidden agents |
//! | `accounts` | JSON | unset | yes | Saved Claude Code accounts |
//! | `untrackedCostAccount` | any | unset | yes | Unused (only in the migration list) |
//! | `antigravityAccounts` | JSON | unset | yes | Saved Antigravity accounts |
//! | `claudeDesktopAccounts` | list of strings | `[]` | yes | Claude Desktop accounts seen |
//! | `codexAccounts` | JSON | unset | yes | Saved Codex accounts |
//! | `limitMode` | `off`/`notify`/`notifyAndSwitch` | `notifyAndSwitch` | yes | Claude limit watching |
//! | `limitSwitchAt` | int | 95 | yes | Percent that triggers a switch (reads are held to 80 to 99) |
//! | `codexLimitMode` | `off`/`notify`/`notifyAndSwitch` | `notify` | yes | Codex limit watching |
//! | `antigravityLimitMode` | `off`/`notify` | `notify` | yes | Antigravity limit watching |
//! | `codexNetwork` | bool | true | yes | Network open in Codex turns |
//! | `migratedFromAwake` | bool | false | no | The migration ran |
//! | `chatClosedSessions` | dict string to number | `{}` | no | Sessions closed in Chat |
//! | `chatInspector` | bool | false | no | Details panel open |
//! | `codexPicks` | JSON | unset | no | Model and effort per Codex session |
//! | `chatProjectFolders` | list of strings | `[]` | no | Recent New Session folders (30) |
//! | `newSessionChoice` | string | unset | no | Account last picked in New Session |
//! | `chatOpen` | bool | false | no | Chat window was open at quit |
//! | `chatSelection` | string | unset | no | Session selected in Chat |
//! | `chatPageWidth` | number | 520 | no | Page panel width |
//! | `vaultReminderSent` | dict | `{}` | no | Vault reminders already sent |
//! | `bridgeEnabled` | bool | false | no | Phone bridge switch |
//! | `debugAccessCertsFile` | string | unset | no | Debug: Cloudflare Access certs file |
//!
//! `launchAtLogin` and `Keep Mac Awake`'s assertion are not defaults: the first is the system's login
//! item, the second is runtime state.

pub mod dirs;
pub mod keys;
pub mod migrate;
pub mod store;

pub use dirs::{data_dir, settings_file, DirEnv, Os};
pub use keys::{spec, Fallback, KeySpec, Kind, KEYS};
pub use migrate::{import, migrate, DefaultsExport, Migrated, Report};
pub use store::{Settings, SettingsError};
