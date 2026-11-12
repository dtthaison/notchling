//! Delegate and second opinions (Swift `Delegate/`, `SecondOpinionStore`): `notchling delegate
//! <codex|agy> [--model --effort --folder --out --write --session --timeout --json] -- <prompt>`
//! hands a job to another tool on this Mac and waits for it; `notchling delegate-models` lists
//! what each tool offers.
//!
//! The command is a client mode of the binary. It needs no daemon, and it is not behind the
//! daemon's `acting` switch: it runs a tool only because the agent that called it asked, in the
//! tool's read-only mode unless `--write` is given, and it never touches a session it did not
//! start (`--session` continues one the caller names). Output text, the `--json` shape and the exit
//! codes (0 done, 1 failed, 124 timed out, 2 a command line that can't run) are the Swift
//! command's.
//!
//! | Part | Where |
//! | --- | --- |
//! | The command line, nesting limit (`NOTCHLING_DELEGATE_DEPTH`, 2) | [`options`] |
//! | Running a job and printing it; `delegate-models` | [`command`] |
//! | One Codex turn (read-only sandbox unless `--write`) | [`codex_run`] |
//! | One Antigravity turn over ACP (permissions declined and listed; edits only with `--write`) | [`agy_run`] |
//! | A folder Claude Code trusts is added to agy's trusted workspaces before it starts | [`agy_trust`] |
//! | Waiting for agy's subagents, by file events | [`progress`] |
//! | The record a delegation leaves for Chat (a subagent of the session that handed it over) | [`record`] |
//! | Generated media pickup and `--out` copies (no traversal, no symlink out) | [`media`] |
//! | agy's model and thinking level | [`agy_pick`] |
//! | The result as text and `--json` | [`outcome`] |
//! | Everything the machine supplies, as parameters; the real ones | [`deps`], [`real`] |
//! | Second opinions: prompt, arguments, answer, the card | [`second_opinion`] |
//! | Their store (a [`second_opinion_store::Host`] starts the command) and the real host | [`second_opinion_store`], [`second_opinion_host`] |
//! | The `notchling-delegate` skill's text (E13 installs it, not this milestone) | [`skill`] |
//!
//! No test runs `codex`, `agy` or `claude`: the tests use the scripted tool processes in
//! [`crate::tools`], the recording of a real `agy acp` exchange, and `/bin/sh` scripts.

pub mod agy_pick;
pub mod agy_run;
pub mod agy_trust;
pub mod codex_run;
pub mod command;
pub mod deps;
pub mod media;
pub mod options;
pub mod outcome;
pub mod progress;
pub mod real;
pub mod record;
pub mod second_opinion;
pub mod second_opinion_host;
pub mod second_opinion_store;
pub mod skill;
#[cfg(test)]
pub(crate) mod test_support;
