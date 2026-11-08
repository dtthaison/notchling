//! E6a: Codex turns. `codex exec --json` runs one turn per process (`resume <id>` after the first)
//! and exits when it ends; Chat reads the conversation from the session's rollout file, so what
//! lives here is the command line, the turn's events, the queue of messages waiting behind a turn,
//! Stop (SIGINT) and delete, plus the slash commands and models Codex's own picker offers.
//!
//! # Public API
//!
//! | Part | Where |
//! | --- | --- |
//! | Argv, environment, the `codex` binary, how a turn's exit is judged | [`plan`] (`CodexRunner.arguments`) |
//! | One `exec --json` line | [`events::parse_line`] |
//! | Starting a tool and reading it as it runs | [`process::ToolSpawner`], [`process::RealSpawner`]; a scripted one in [`fake`] |
//! | Running turns, the queue, Stop | [`runs::CodexRuns`] |
//! | Slash commands and what each does | [`commands`] (`CodexCommands`) |
//! | The models Codex offers | [`models`] (`CodexModels`) |
//! | Rollouts as sidebar rows | [`list`] (`CodexSessions.list`, moved from the app) |
//!
//! Nothing here runs a tool unless a caller hands it a [`process::ToolSpawner`]; the daemon does
//! that only while its `acting` switch is on.

pub mod commands;
pub mod events;
pub mod fake;
pub mod list;
pub mod models;
pub mod plan;
pub mod process;
pub mod runs;
