//! Other tools the app runs besides Claude Code: Codex (E6a, [`codex`]). Antigravity (E6b) joins
//! it later. Nothing here runs unless a caller hands a plan to a spawner, and the daemon does that
//! only while its `acting` switch is on.

pub mod codex;
