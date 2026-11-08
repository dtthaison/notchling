//! E15, Visualize: what the phone's Visualize tabs show for a session (ConsoleCore's `VizEngine`,
//! `VizSpec`, `VisualizeSuggester`, `TranscriptFacts`, `WebURLResolver` and `GitReader`).
//!
//! The specs are the generated `agentconsole.v1` types (`notchling_protocol::generated::VizSpec`);
//! the bridge writes them with sorted keys, as the Swift `VizSpec.json()` did.
//!
//! | Part | What it does |
//! | --- | --- |
//! | [`facts`] | One pass over a transcript: turns and their calls, edited files, mermaid blocks, token usage, local URLs, device names. |
//! | [`url`] | A small URL reader for the local pages the Web tab names. |
//! | [`web_url`] | Which local page the Web tab opens: the transcript's URLs, then `.claude/launch.json`. The listening check is a closure, so a test needs no server. |
//! | [`suggest`] | The tab worth opening first, from the folder and what the transcript said. |
//! | [`engine`] | [`VizEngine`]: the flow, diagram, chart and entity specs, with a cache of parsed transcripts. |
//!
//! # Rulings
//!
//! - The entity spec (Prisma, SQL and Drift schemas) needs `SchemaParsers`, which is not ported yet.
//!   [`engine::Schemas`] is the seam; [`engine::NoSchemas`] answers with an empty state that says
//!   this computer does not read schemas yet.
//! - `git diff --stat` runs through a `control::Launcher`, as every other `git` call does; the
//!   branch is read from `.git/HEAD` with no process.

pub mod engine;
pub mod facts;
pub mod git;
pub mod suggest;
pub mod url;
pub mod web_url;

pub use engine::{GitSnapshot, NoSchemas, Scan, Schemas, VizBundle, VizEngine};
pub use facts::{MermaidBlock, TranscriptFacts};
pub use suggest::{git_branch, VizSignals, VizTab};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_git;
#[cfg(test)]
mod tests_urls;
