//! Other tools the app runs besides Claude Code: Codex (E6a, [`codex`]) and Antigravity CLI
//! (E6b, [`agy`] over the Agent Client Protocol in [`acp`]). Nothing here runs unless a caller hands
//! a plan to a spawner, and the daemon does that only while its `acting` switch is on.

pub mod acp;
pub mod agy;
pub mod codex;
