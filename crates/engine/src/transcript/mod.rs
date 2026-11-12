//! E1: transcript readers (Claude Code, Codex, Antigravity), subagents, background shells.
//!
//! Every reader is bounded: it reads the last 2 MB (notch) or 4 MB (Chat) of a file, skips lines over
//! 100 KB (screenshots, huge tool output) and never reads a file whole. Nothing here writes a file or
//! starts a process. Times are `f64` seconds since the epoch ([`model::Timestamp`]); counts are in
//! Unicode scalars where the Swift app counted grapheme clusters.
//!
//! # Public API
//!
//! | Function | Returns |
//! | --- | --- |
//! | [`conversation::read`]`(path: &str)` | [`model::Conversation`] of a Claude Code session transcript: items (user/assistant text, thinking, tool steps with output and Edit/Write diffs, turn ends, notices, slash-command and `!` command output), tasks, facts, `queued` messages, `finished_agents`, background `shells`. Incremental: a repeat call parses only appended lines. An unreadable file gives `Conversation::default()`. |
//! | [`conversation::read_subagent`]`(path)` / [`subagents::read`]`(path)` | The same for a subagent's own sidechain transcript. |
//! | [`conversation::Parser`] | The line-at-a-time parser behind `read`: `Parser::new()`, `consume(&[u8])`, `conversation()`. |
//! | [`tail::read_lines`]`(path, max_bytes)` | `io::Result<Vec<String>>`: the file's last `max_bytes` as lines, the cut first line dropped. |
//! | [`tail::Cursors`] / [`tail::LineParser`] | The incremental reader: `Cursors::new(tail_bytes, limit, render)` and `read(path) -> Option<O>`. The conversation readers keep `Arc<Conversation>` in them and expose it as `conversation::read_shared`, `read_subagent_shared`, `codex::read_shared` and `subagents::read_shared`: an unchanged file gives the same `Arc`, so a caller tells "nothing changed" by `Arc::ptr_eq`; `read` and friends return an owned copy. |
//! | [`snapshot::read`]`(path)` | [`snapshot::Snapshot`] for the notch: the last lines, `last_prompt`, `activity`, the pending [`snapshot::PendingQuestion`], `subagents_active`. |
//! | [`errors::limit_hit_in_file`], [`errors::sleep_cut_in_file`], [`errors::tool_finished`] | What a transcript's end says. |
//! | [`locate::project_folder`], [`locate::find_transcript`], [`locate::job_transcript`], [`locate::successor`] | Where a session's transcript is, a job that moved into a worktree, the file a `/clear` started. |
//! | [`codex::read`]`(path)` / [`codex::meta`] / [`codex::turn_open`] | A Codex rollout as a `Conversation`, its id/cwd/title, and whether its last turn is still open. |
//! | [`acp::Transcript`] | An Antigravity (ACP) conversation built from `session/update` payloads: `apply`, `begin_turn`, `end_turn`, `fail_turn`, `conversation`. |
//! | [`subagents::list`]`(transcript, &finished, now)` | `Vec<Subagent>` of a session, with [`subagents::Status`]; [`subagents::console_list`] is the phone bridge's view. |
//! | [`shells::launch`], [`shells::state`], [`shells::parse_writers`] | Background shell launch parsing and state (the `lsof` call itself is the caller's). |
//! | [`chat::read`]`(path, is_main, limit)` | [`chat::ParseResult`]: the phone's message view (ConsoleCore `ChatTranscript`). |

pub mod acp;
pub mod bounded;
pub mod chat;
pub mod codex;
pub mod conversation;
pub mod errors;
pub mod locate;
pub mod model;
pub mod shells;
pub mod snapshot;
pub mod subagents;
pub mod tail;
pub mod tally;
pub mod text;
pub mod util;
