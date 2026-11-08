//! Codex and Antigravity rows beside Claude Code's (`NotchForeign`, `ForeignSessions`): the
//! Antigravity list ([`AgyList`], over a copy of its summaries database, never the original), what the notch
//! shows of each tool's rows (12 hours of Done rows, 8 per tool), and a conversation as the notch's
//! lines. Codex rows come from the engine's rollout listing, as Chat's do.

use std::path::Path;

use notchling_engine::bridge::foreign::tool_summary;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::tools::agy::summaries::Row;
use notchling_engine::transcript::model::{Conversation, ItemKind};
use notchling_engine::transcript::snapshot::{ChatLine, LineKind, Snapshot};

pub use super::agy_list::AgyList;
use super::requests::RowTool;

/// Done rows older than this are left out of the notch (Chat lists 14 days).
pub const MAX_AGE_MS: i64 = 12 * 3600 * 1000;
/// At most this many rows of each tool in the notch.
pub const PER_TOOL: usize = 8;
/// A reply shows at most this many characters in the notch.
const REPLY_CHARS: usize = 3000;

/// An Antigravity summaries row as a session row (`AntigravitySessions.list`).
pub fn agy_agent(row: &Row) -> Agent {
    Agent {
        id: row.row_id(),
        name: if row.title.trim().is_empty() {
            "Antigravity Session".to_owned()
        } else {
            row.title.clone()
        },
        project: Path::new(&row.folder)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        cwd: row.folder.clone(),
        group: if row.running {
            AgentGroup::Working
        } else {
            AgentGroup::Completed
        },
        label: if row.running {
            "Working"
        } else {
            "Antigravity"
        }
        .to_owned(),
        detail: None,
        since_ms: (row.modified * 1000.0) as i64,
        target: AgentTarget::Resume {
            session_id: row.id.clone(),
            cwd: row.folder.clone(),
        },
        suggested_reply: None,
        transcript_path: None,
        host: "Antigravity".to_owned(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
    }
}

/// The rows the notch shows of the other tools: newest first, at most [`PER_TOOL`] of each, and
/// Done rows only from the last [`MAX_AGE_MS`]. Claude Code rows pass through untouched.
pub fn visible(rows: Vec<Agent>, now_ms: i64) -> Vec<Agent> {
    let (mut foreign, claude): (Vec<Agent>, Vec<Agent>) = rows
        .into_iter()
        .partition(|a| RowTool::of(&a.id) != RowTool::Claude);
    foreign.sort_by(|a, b| b.since_ms.cmp(&a.since_ms).then(a.id.cmp(&b.id)));
    let (mut codex, mut agy) = (0usize, 0usize);
    let kept = foreign.into_iter().filter(|a| {
        if a.group == AgentGroup::Completed && now_ms - a.since_ms >= MAX_AGE_MS {
            return false;
        }
        let count = match RowTool::of(&a.id) {
            RowTool::Codex => &mut codex,
            RowTool::Agy => &mut agy,
            RowTool::Claude => return true,
        };
        if *count >= PER_TOOL {
            return false;
        }
        *count += 1;
        true
    });
    claude.into_iter().chain(kept).collect()
}

/// A conversation as the notch's detail bubbles: your messages, the replies, and one dim line for
/// the tools between them. Thinking, turn ends and notices stay in Chat. Ids count from the start,
/// so the tail keeps the ids it had.
pub fn lines(conversation: &Conversation, limit: usize) -> Vec<ChatLine> {
    let mut out: Vec<ChatLine> = Vec::new();
    for item in &conversation.items {
        let (kind, text) = match &item.kind {
            ItemKind::User { text, .. } => (LineKind::User, text.clone()),
            ItemKind::Assistant { text } => {
                let start = text
                    .char_indices()
                    .nth_back(REPLY_CHARS - 1)
                    .map_or(0, |(i, _)| i);
                (LineKind::Assistant, text[start..].to_owned())
            }
            ItemKind::Tools(calls) if !calls.is_empty() => (LineKind::Tools, tool_summary(calls)),
            _ => continue,
        };
        out.push(ChatLine {
            id: out.len(),
            kind,
            text,
        });
    }
    let skip = out.len().saturating_sub(limit);
    out.split_off(skip)
}

/// The notch's snapshot of a Codex or Antigravity conversation.
pub fn snapshot(conversation: &Conversation) -> Snapshot {
    let lines = lines(conversation, 60);
    Snapshot {
        last_prompt: lines
            .iter()
            .rev()
            .find(|l| l.kind == LineKind::User)
            .map(|l| l.text.clone()),
        activity: conversation.activity.clone(),
        lines,
        question: None,
        subagents_active: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::transcript::model::{ChatItem, ToolCall, ToolKind};

    fn row(id: &str, since_ms: i64, group: AgentGroup) -> Agent {
        Agent {
            group,
            since_ms,
            ..agy_agent(&Row {
                id: id.to_owned(),
                title: "T".into(),
                folder: "/w/p".into(),
                modified: 0.0,
                running: false,
            })
        }
    }

    #[test]
    fn a_summaries_row_becomes_an_antigravity_agent() {
        let a = agy_agent(&Row {
            id: "abc".into(),
            title: "".into(),
            folder: "/Users/me/work/api".into(),
            modified: 1_700_000_000.5,
            running: true,
        });
        assert_eq!(a.id, "agy:abc");
        assert_eq!(a.name, "Antigravity Session");
        assert_eq!(a.project, "api");
        assert_eq!(
            (a.group, a.label.as_str()),
            (AgentGroup::Working, "Working")
        );
        assert_eq!(a.since_ms, 1_700_000_000_500);
        assert_eq!(
            a.target,
            AgentTarget::Resume {
                session_id: "abc".into(),
                cwd: "/Users/me/work/api".into()
            }
        );
    }

    #[test]
    fn the_notch_keeps_recent_rows_a_few_per_tool_and_every_claude_row() {
        let now = 100 * 3600 * 1000;
        let mut rows = vec![Agent {
            id: "aa11bb22".into(),
            since_ms: 0,
            ..row("c", 0, AgentGroup::Completed)
        }];
        for i in 0..10 {
            rows.push(Agent {
                id: format!("codex:{i}"),
                ..row("x", now - i * 60_000, AgentGroup::Completed)
            });
        }
        rows.push(row("old", now - MAX_AGE_MS, AgentGroup::Completed));
        rows.push(row(
            "old-working",
            now - 2 * MAX_AGE_MS,
            AgentGroup::Working,
        ));
        let kept = visible(rows, now);
        let ids: Vec<&str> = kept.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids[0], "aa11bb22", "Claude rows pass through, however old");
        assert_eq!(
            ids.iter().filter(|i| i.starts_with("codex:")).count(),
            PER_TOOL
        );
        assert_eq!(ids[1], "codex:0", "newest first");
        assert!(ids.contains(&"agy:old-working"), "a working row stays");
        assert!(!ids.contains(&"agy:old"));
    }

    #[test]
    fn lines_show_prompts_replies_and_a_tool_summary() {
        let long = "x".repeat(REPLY_CHARS + 10);
        let conversation = Conversation {
            items: vec![
                ChatItem {
                    id: "1".into(),
                    kind: ItemKind::user("fix it"),
                    date: None,
                },
                ChatItem {
                    id: "2".into(),
                    kind: ItemKind::Thinking {
                        seconds: 1.0,
                        text: String::new(),
                    },
                    date: None,
                },
                ChatItem {
                    id: "3".into(),
                    kind: ItemKind::Tools(vec![ToolCall::new(
                        "t",
                        "exec",
                        ToolKind::Shell,
                        "ls".into(),
                        "ls".into(),
                    )]),
                    date: None,
                },
                ChatItem {
                    id: "4".into(),
                    kind: ItemKind::Tools(vec![]),
                    date: None,
                },
                ChatItem {
                    id: "5".into(),
                    kind: ItemKind::Assistant { text: long.clone() },
                    date: None,
                },
            ],
            activity: Some("Running ls".into()),
            ..Conversation::default()
        };
        let all = lines(&conversation, 60);
        assert_eq!(all.len(), 3);
        assert_eq!(
            (all[0].kind, all[0].text.as_str()),
            (LineKind::User, "fix it")
        );
        assert_eq!(all[1].text, "Ran 1 shell command");
        assert_eq!(all[2].text.chars().count(), REPLY_CHARS);
        let tail = lines(&conversation, 2);
        assert_eq!(tail.iter().map(|l| l.id).collect::<Vec<_>>(), [1, 2]);
        let snap = snapshot(&conversation);
        assert_eq!(snap.last_prompt.as_deref(), Some("fix it"));
        assert_eq!(snap.activity.as_deref(), Some("Running ls"));
        assert_eq!(snap.last_reply().map(str::len), Some(REPLY_CHARS));
    }
}
