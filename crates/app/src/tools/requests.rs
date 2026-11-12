//! The Codex and Antigravity ops as the app sends them: the shapes from the daemon's `codex_ops`
//! and `agy_ops` module docs, built here so they are tested and the socket only carries them.
//! Row ids are Chat's: `codex:<session>`, `agy:<session>`; a draft (`codex-new:<uuid>`,
//! `agy-new:<uuid>`) sends `draft_folder` instead and the reply names the real row.

use notchling_engine::transcript::model::Conversation;
use notchling_local_api::ops;
use serde_json::{json, Value};

use crate::acting::Request;

/// A Codex row's id starts with this (`CodexSessions.agentID`).
pub const CODEX_PREFIX: &str = "codex:";
/// An Antigravity row's id starts with this.
pub const AGY_PREFIX: &str = "agy:";
/// Drafts: a new session not started yet, keyed by a uuid.
pub const CODEX_DRAFT_PREFIX: &str = "codex-new:";
pub const AGY_DRAFT_PREFIX: &str = "agy-new:";

/// Which tool a row belongs to, by its id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowTool {
    Claude,
    Codex,
    Agy,
}

impl RowTool {
    pub fn of(row_id: &str) -> RowTool {
        if row_id.starts_with(CODEX_PREFIX) || row_id.starts_with(CODEX_DRAFT_PREFIX) {
            RowTool::Codex
        } else if row_id.starts_with(AGY_PREFIX) || row_id.starts_with(AGY_DRAFT_PREFIX) {
            RowTool::Agy
        } else {
            RowTool::Claude
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RowTool::Claude => "Claude Code",
            RowTool::Codex => "Codex",
            RowTool::Agy => "Antigravity",
        }
    }
}

/// Whether a row is a draft: its first message starts the session.
pub fn is_draft(row_id: &str) -> bool {
    row_id.starts_with(CODEX_DRAFT_PREFIX) || row_id.starts_with(AGY_DRAFT_PREFIX)
}

/// A Codex session's model and effort for its next turns; `None` is Codex's own default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pick {
    pub model: Option<String>,
    pub effort: Option<String>,
}

fn with_pick(mut args: Value, pick: &Pick) -> Value {
    if let Some(model) = &pick.model {
        args["model"] = json!(model);
    }
    if let Some(effort) = &pick.effort {
        args["effort"] = json!(effort);
    }
    args
}

/// `codex.send` to a session; while its turn runs the daemon queues the message.
pub fn codex_send(id: &str, text: &str, pick: &Pick) -> Request {
    Request {
        op: ops::CODEX_SEND,
        args: with_pick(json!({ "id": id, "text": text }), pick),
    }
}

/// `codex.send` that starts a new session in `folder` from the draft row `draft`: the daemon
/// echoes the draft in the event that names the session when that takes over its 4.5 s.
pub fn codex_send_draft(draft: &str, folder: &str, text: &str, pick: &Pick) -> Request {
    Request {
        op: ops::CODEX_SEND,
        args: with_pick(
            json!({ "draft_folder": folder, "text": text, "draft": draft }),
            pick,
        ),
    }
}

pub fn codex_stop(id: &str) -> Request {
    Request {
        op: ops::CODEX_STOP,
        args: json!({ "id": id }),
    }
}

pub fn codex_delete(id: &str) -> Request {
    Request {
        op: ops::CODEX_DELETE,
        args: json!({ "id": id }),
    }
}

/// `agy.open`: starts the session's `agy acp` and loads it (acting).
pub fn agy_open(id: &str) -> Request {
    Request {
        op: ops::AGY_OPEN,
        args: json!({ "id": id }),
    }
}

pub fn agy_send(id: &str, text: &str) -> Request {
    Request {
        op: ops::AGY_SEND,
        args: json!({ "id": id, "text": text }),
    }
}

pub fn agy_send_draft(folder: &str, text: &str) -> Request {
    Request {
        op: ops::AGY_SEND,
        args: json!({ "draft_folder": folder, "text": text }),
    }
}

pub fn agy_cancel(id: &str) -> Request {
    Request {
        op: ops::AGY_CANCEL,
        args: json!({ "id": id }),
    }
}

pub fn agy_set_option(id: &str, option: &str, value: &str) -> Request {
    Request {
        op: ops::AGY_SET_OPTION,
        args: json!({ "id": id, "option": option, "value": value }),
    }
}

/// Answers a permission with one of Antigravity's own options; `None` cancels it.
pub fn agy_permission(id: &str, rpc_id: &str, option_id: Option<&str>) -> Request {
    let mut args = json!({ "id": id, "rpc_id": rpc_id });
    if let Some(option) = option_id {
        args["option_id"] = json!(option);
    }
    Request {
        op: ops::AGY_PERMISSION,
        args,
    }
}

/// `agy.conversation`, a read (sent through `Actions::read`): `peek` doesn't count as use, for
/// a list read every few seconds.
pub fn agy_conversation(id: &str, peek: bool) -> Request {
    Request {
        op: ops::AGY_CONVERSATION,
        args: json!({ "id": id, "peek": peek }),
    }
}

/// `agy.state`, a read.
pub fn agy_state(id: &str) -> Request {
    Request {
        op: ops::AGY_STATE,
        args: json!({ "id": id }),
    }
}

/// What `agy.conversation` answers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgyConversation {
    pub open: bool,
    pub loading: bool,
    pub working: bool,
    pub load_error: Option<String>,
    pub conversation: Conversation,
    /// The daemon sent only the newest items of a long session (it answers in one 1 MiB line).
    pub truncated: bool,
    /// Another `agy` holds the session (a delegation): what to say; it is read only here.
    pub elsewhere: Option<String>,
}

impl AgyConversation {
    /// Whether another `agy` (a delegation) holds the session and is mid-turn: the daemon reads
    /// that from Antigravity's log. The live set only tracks this daemon's own turns, so without
    /// this the session would look idle. It is not a turn Stop can reach.
    pub fn held_working(&self) -> bool {
        self.elsewhere.is_some() && self.working
    }
}

/// Reads an `agy.conversation` reply; `None` when it isn't one.
pub fn parse_conversation(reply: &Value) -> Option<AgyConversation> {
    let open = reply.get("open")?.as_bool()?;
    let conversation = match reply.get("conversation") {
        Some(Value::Null) | None => Conversation::default(),
        Some(value) => serde_json::from_value(value.clone()).ok()?,
    };
    Some(AgyConversation {
        open,
        loading: reply
            .get("loading")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        working: reply
            .get("working")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        load_error: reply
            .get("loadError")
            .and_then(Value::as_str)
            .map(str::to_owned),
        conversation,
        truncated: reply
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        elsewhere: reply
            .get("elsewhere")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// The row id a `codex.send` or `agy.send` reply names: a draft's first message gives the real
/// session's row.
pub fn sent_row(reply: &Value) -> Option<String> {
    reply.get("id")?.as_str().map(str::to_owned)
}

/// What a `codex.send` reply says about the message: started a turn, or waits behind one (how
/// many are ahead of it, counting it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sent {
    Started,
    Queued(usize),
    /// A new session Codex hasn't named yet: the real row comes in an event naming the draft.
    Starting,
}

pub fn sent_state(reply: &Value) -> Sent {
    match reply.get("state").and_then(Value::as_str) {
        Some("starting") => Sent::Starting,
        Some("queued") => Sent::Queued(
            reply
                .get("waiting")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .max(1) as usize,
        ),
        _ => Sent::Started,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_are_told_apart_by_the_row_id() {
        assert_eq!(RowTool::of("codex:abc"), RowTool::Codex);
        assert_eq!(RowTool::of("codex-new:u1"), RowTool::Codex);
        assert_eq!(RowTool::of("agy:abc"), RowTool::Agy);
        assert_eq!(RowTool::of("agy-new:u1"), RowTool::Agy);
        assert_eq!(RowTool::of("aa11bb22"), RowTool::Claude);
        assert!(is_draft("codex-new:u1") && is_draft("agy-new:u1"));
        assert!(!is_draft("codex:abc"));
        assert_eq!(RowTool::Agy.name(), "Antigravity");
    }

    #[test]
    fn codex_requests_have_the_daemons_shapes() {
        let plain = codex_send("codex:s1", "fix it", &Pick::default());
        assert_eq!(plain.op, "codex.send");
        assert_eq!(plain.args, json!({"id": "codex:s1", "text": "fix it"}));
        let picked = Pick {
            model: Some("gpt-6.1-sol".into()),
            effort: Some("high".into()),
        };
        assert_eq!(
            codex_send_draft("codex-new:u1", "/w/api", "start", &picked).args,
            json!({"draft_folder": "/w/api", "text": "start", "draft": "codex-new:u1",
                   "model": "gpt-6.1-sol", "effort": "high"})
        );
        let model_only = Pick {
            model: Some("m".into()),
            effort: None,
        };
        assert_eq!(
            codex_send("codex:s1", "x", &model_only).args,
            json!({"id": "codex:s1", "text": "x", "model": "m"})
        );
        assert_eq!(
            sent_state(&json!({"state": "starting", "draft": "codex-new:u1"})),
            Sent::Starting
        );
        assert_eq!(sent_row(&json!({"state": "starting"})), None);
        assert_eq!(codex_stop("codex:s1").op, "codex.stop");
        assert_eq!(codex_delete("codex:s1").args, json!({"id": "codex:s1"}));
    }

    #[test]
    fn agy_requests_have_the_daemons_shapes() {
        assert_eq!(agy_open("agy:s1").op, "agy.open");
        assert_eq!(
            agy_send("agy:s1", "go").args,
            json!({"id": "agy:s1", "text": "go"})
        );
        assert_eq!(
            agy_send_draft("/w/api", "go").args,
            json!({"draft_folder": "/w/api", "text": "go"})
        );
        assert_eq!(agy_cancel("agy:s1").op, "agy.cancel");
        assert_eq!(
            agy_set_option("agy:s1", "model", "gemini-3").args,
            json!({"id": "agy:s1", "option": "model", "value": "gemini-3"})
        );
        assert_eq!(
            agy_permission("agy:s1", "7", Some("allow")).args,
            json!({"id": "agy:s1", "rpc_id": "7", "option_id": "allow"})
        );
        assert_eq!(
            agy_permission("agy:s1", "7", None).args,
            json!({"id": "agy:s1", "rpc_id": "7"})
        );
        assert_eq!(
            agy_conversation("agy:s1", true).args,
            json!({"id": "agy:s1", "peek": true})
        );
        assert_eq!(agy_state("agy:s1").op, "agy.state");
    }

    #[test]
    fn only_a_held_session_with_a_turn_going_is_working_elsewhere() {
        let note = "A delegation is running this session. Read only here until it ends.";
        let held = |working: bool| {
            parse_conversation(&json!({"open": false, "working": working,
                "conversation": {"items": []}, "elsewhere": note}))
            .unwrap()
        };
        assert!(held(true).held_working());
        assert!(!held(false).held_working(), "the delegation answered");
        // A turn this daemon runs is the runners' to report, and Stop's.
        let own = parse_conversation(&json!({"open": true, "working": true,
            "conversation": {"items": []}}))
        .unwrap();
        assert!(!own.held_working());
    }

    #[test]
    fn replies_are_read() {
        let reply = json!({"id": "agy:s1", "open": true, "loading": false, "working": true,
            "loadError": null, "conversation": {"items": [], "activity": "Editing"}});
        let c = parse_conversation(&reply).unwrap();
        assert!(c.open && c.working && !c.loading);
        assert_eq!(c.conversation.activity.as_deref(), Some("Editing"));
        assert_eq!(c.elsewhere, None);
        let held = json!({"id": "agy:s1", "open": false, "working": true, "conversation": {"items": []},
            "elsewhere": "A delegation is running this session. Read only here until it ends."});
        assert_eq!(
            parse_conversation(&held).unwrap().elsewhere.as_deref(),
            Some("A delegation is running this session. Read only here until it ends.")
        );
        assert!(!c.truncated, "an older daemon never says");
        // A newer daemon's extra fields are read, and a long session says it was cut.
        let cut = parse_conversation(&json!({"open": true, "truncated": true, "omitted": 40,
            "conversation": {"items": []}}))
        .unwrap();
        assert!(cut.truncated);
        let closed = parse_conversation(&json!({"open": false})).unwrap();
        assert_eq!(closed.conversation, Conversation::default());
        assert_eq!(parse_conversation(&json!({"items": []})), None);

        assert_eq!(
            sent_row(&json!({"id": "codex:new1", "state": "started"})).as_deref(),
            Some("codex:new1")
        );
        assert_eq!(sent_state(&json!({"state": "started"})), Sent::Started);
        assert_eq!(
            sent_state(&json!({"state": "queued", "waiting": 2})),
            Sent::Queued(2)
        );
        assert_eq!(sent_state(&json!({"state": "queued"})), Sent::Queued(1));
    }
}
