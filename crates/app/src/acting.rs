//! Whether the app may act on sessions, and what it asks the daemon to do (Milestone 3).
//!
//! The daemon ships with acting switched off: every acting op answers `forbidden` with
//! [`OFF_MESSAGE`] until the maintainer turns it on. The app never guesses: an action control is
//! live only while the daemon is attached and has said acting is on ([`Acting::control`]). The
//! daemon's own refusal or a lost connection turns every control off again ([`after_error`]).
//!
//! The op names and their arguments are the plan's (`docs/superpowers/plans/
//! 2026-11-08-phase7-m3-daemon-acts.md`, "Daemon ops"); [`Request`] builds them, pure, so the
//! shapes are tested here and the socket only carries them.

use notchling_engine::transcript::snapshot::PendingQuestion;
use notchling_local_api::{ClientError, ErrorCode};
use serde_json::{json, Value};

/// What the daemon says, word for word, while acting is off.
pub const OFF_MESSAGE: &str =
    "acting is off: the Swift app owns sessions until the maintainer turns this on";
/// What a control says while there is no daemon to ask.
pub const DETACHED_MESSAGE: &str = "Notchling's daemon isn't attached, so nothing can be sent";

/// The acting ops (app tier) and the status read.
pub mod ops {
    pub const DAEMON_STATUS: &str = "daemon.status";
    pub const SETTINGS_SET_ACTING: &str = "settings.set_acting";
    pub const SESSION_REPLY: &str = "session.reply";
    pub const SESSION_ANSWER: &str = "session.answer";
    pub const SESSION_PERMISSION: &str = "session.permission";
    pub const SESSION_INTERRUPT: &str = "session.interrupt";
    pub const SESSION_STOP: &str = "session.stop";
    pub const SESSION_REMOVE: &str = "session.remove";
    pub const SESSION_TAKE_BACK: &str = "session.take_back";
    pub const SESSION_NEW: &str = "session.new";
    pub const FOLDER_TRUST: &str = "folder.trust";
    pub const SESSION_MOVE: &str = "session.move";
    pub const SESSION_COMMAND: &str = "session.command";
}

/// Whether actions can be sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acting {
    /// The daemon is attached and acting is on.
    On,
    /// The daemon is attached and acting is off (the default), or it hasn't said.
    Off,
    /// No daemon to ask: down, not trusted, a render or a fixture root.
    Detached,
}

/// How an action control is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Control {
    pub enabled: bool,
    /// Why it's disabled, shown on hover.
    pub tooltip: Option<&'static str>,
}

impl Acting {
    /// Every action control follows this: live only while acting is on.
    pub fn control(self) -> Control {
        Control {
            enabled: self == Acting::On,
            tooltip: self.message(),
        }
    }

    /// Why nothing can be sent, or `None` while it can.
    pub fn message(self) -> Option<&'static str> {
        match self {
            Acting::On => None,
            Acting::Off => Some(OFF_MESSAGE),
            Acting::Detached => Some(DETACHED_MESSAGE),
        }
    }

    /// The Settings sheet's read-only line.
    pub fn word(self) -> &'static str {
        match self {
            Acting::On => "On",
            Acting::Off => "Off",
            Acting::Detached => "Daemon not attached",
        }
    }
}

/// What acting is, given whether the daemon is attached and what it last said. Detached wins.
pub fn effective(attached: bool, said: Acting) -> Acting {
    if attached {
        said
    } else {
        Acting::Detached
    }
}

/// Reads `daemon.status`'s `acting` (`true`, or `"on"`): anything else, a missing field included,
/// is off, since off is the daemon's default.
pub fn from_status(status: &Value) -> Acting {
    match status.get("acting") {
        Some(Value::Bool(true)) => Acting::On,
        Some(Value::String(s)) if s == "on" => Acting::On,
        _ => Acting::Off,
    }
}

/// What a failed call says about acting: the daemon's off refusal turns it off; a connection that
/// is gone or never was detaches it; any other refusal (a bad id, a dialog open) changes nothing.
pub fn after_error(error: &ClientError) -> Option<Acting> {
    match error {
        ClientError::Api(e) if e.code == ErrorCode::Forbidden && is_off(&e.message) => {
            Some(Acting::Off)
        }
        ClientError::Api(_) | ClientError::Protocol(_) => None,
        ClientError::NotRunning(_)
        | ClientError::Timeout
        | ClientError::Closed
        | ClientError::Unsupported
        | ClientError::UntrustedServer(_)
        | ClientError::Io(_) => Some(Acting::Detached),
    }
}

/// The daemon's off refusal, however it is prefixed.
pub fn is_off(message: &str) -> bool {
    message.contains("acting is off")
}

/// Claude Code refused a folder it hasn't trusted (`ReplySender.isUntrusted`).
pub fn is_untrusted(message: &str) -> bool {
    message.contains("not trusted")
}

/// One answer to one question: options by their 1-based number (one for single select), or text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Options(Vec<usize>),
    Text(String),
}

impl Answer {
    fn to_value(&self) -> Value {
        match self {
            Answer::Options(picked) => json!(picked),
            Answer::Text(text) => json!(text),
        }
    }
}

/// A permission prompt's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Always,
}

impl Decision {
    pub fn wire(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::Always => "always",
        }
    }
}

/// One op and its arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub op: &'static str,
    pub args: Value,
}

impl Request {
    fn new(op: &'static str, args: Value) -> Request {
        Request { op, args }
    }

    pub fn reply(id: &str, text: &str) -> Request {
        Request::new(ops::SESSION_REPLY, json!({ "id": id, "text": text }))
    }

    pub fn answer(id: &str, answers: &[Answer]) -> Request {
        let answers: Vec<Value> = answers.iter().map(Answer::to_value).collect();
        Request::new(ops::SESSION_ANSWER, json!({ "id": id, "answers": answers }))
    }

    /// Answers the question on a row's card. One a hook holds (`hook:<prompt id>`) is answered by
    /// its prompt id, never the row's, so the answer reaches the prompt the card showed; any other
    /// (read from the transcript or the screen) is typed into the row's dialog, with the question
    /// it was read as so the daemon checks each page.
    pub fn answer_for(row: &str, question: &PendingQuestion, answers: &[Answer]) -> Request {
        if let Some(prompt) = question.tool_use_id.strip_prefix("hook:") {
            return Request::answer(prompt, answers);
        }
        let mut request = Request::answer(row, answers);
        let items: Vec<Value> = question
            .items
            .iter()
            .map(|i| {
                json!({"header": i.header, "question": i.question, "options": i.options,
                       "details": i.details, "multiSelect": i.multi_select})
            })
            .collect();
        request.args["question"] = json!({ "items": items });
        request
    }

    /// `id` is the held prompt's id (`HookRequest::id`), never a row's.
    pub fn permission(id: &str, decision: Decision) -> Request {
        Request::new(
            ops::SESSION_PERMISSION,
            json!({ "id": id, "decision": decision.wire() }),
        )
    }

    pub fn interrupt(id: &str) -> Request {
        Request::new(ops::SESSION_INTERRUPT, json!({ "id": id }))
    }

    pub fn stop(id: &str) -> Request {
        Request::new(ops::SESSION_STOP, json!({ "id": id }))
    }

    pub fn remove(id: &str) -> Request {
        Request::new(ops::SESSION_REMOVE, json!({ "id": id }))
    }

    pub fn take_back(id: &str) -> Request {
        Request::new(ops::SESSION_TAKE_BACK, json!({ "id": id }))
    }

    /// `config_dir` is a lane's folder; `None` is the main login, and the field is left out.
    pub fn new_session(folder: &str, config_dir: Option<&str>, flags: &[String]) -> Request {
        let mut args = json!({ "folder": folder });
        if let Some(dir) = config_dir {
            args["config_dir"] = json!(dir);
        }
        if !flags.is_empty() {
            args["flags"] = json!(flags);
        }
        Request::new(ops::SESSION_NEW, args)
    }

    pub fn trust_folder(folder: &str) -> Request {
        Request::new(ops::FOLDER_TRUST, json!({ "folder": folder }))
    }

    pub fn move_session(id: &str, to_config_dir: &str) -> Request {
        Request::new(
            ops::SESSION_MOVE,
            json!({ "id": id, "to_config_dir": to_config_dir }),
        )
    }

    pub fn command(id: &str, command: &str) -> Request {
        Request::new(
            ops::SESSION_COMMAND,
            json!({ "id": id, "command": command }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_local_api::ApiError;
    use std::io;

    #[test]
    fn only_on_makes_a_control_live() {
        assert_eq!(
            Acting::On.control(),
            Control {
                enabled: true,
                tooltip: None
            }
        );
        let off = Acting::Off.control();
        assert!(!off.enabled);
        assert_eq!(off.tooltip, Some(OFF_MESSAGE));
        let detached = Acting::Detached.control();
        assert!(!detached.enabled);
        assert_eq!(detached.tooltip, Some(DETACHED_MESSAGE));
    }

    #[test]
    fn detached_wins_over_what_the_daemon_said() {
        assert_eq!(effective(false, Acting::On), Acting::Detached);
        assert_eq!(effective(true, Acting::On), Acting::On);
        assert_eq!(effective(true, Acting::Off), Acting::Off);
    }

    #[test]
    fn status_says_on_only_explicitly() {
        assert_eq!(from_status(&json!({"acting": true})), Acting::On);
        assert_eq!(from_status(&json!({"acting": "on"})), Acting::On);
        assert_eq!(from_status(&json!({"acting": false})), Acting::Off);
        assert_eq!(from_status(&json!({"acting": "off"})), Acting::Off);
        assert_eq!(from_status(&json!({"version": "0.1.0"})), Acting::Off);
        assert_eq!(from_status(&Value::Null), Acting::Off);
    }

    #[test]
    fn the_off_refusal_turns_acting_off() {
        let off = ClientError::Api(ApiError::new(ErrorCode::Forbidden, OFF_MESSAGE));
        assert_eq!(after_error(&off), Some(Acting::Off));
        // A forbidden for another reason (the agent tier) says nothing about acting.
        let tier = ClientError::Api(ApiError::new(ErrorCode::Forbidden, "app tier only"));
        assert_eq!(after_error(&tier), None);
        let bad = ClientError::Api(ApiError::bad_request("no session s9"));
        assert_eq!(after_error(&bad), None);
    }

    #[test]
    fn a_lost_daemon_detaches() {
        let gone = [
            ClientError::NotRunning(io::Error::from(io::ErrorKind::NotFound)),
            ClientError::Timeout,
            ClientError::Closed,
            ClientError::Unsupported,
            ClientError::UntrustedServer("another user".into()),
            ClientError::Io(io::Error::other("broken pipe")),
        ];
        for e in &gone {
            assert_eq!(after_error(e), Some(Acting::Detached), "{e}");
        }
    }

    #[test]
    fn untrusted_is_read_from_the_refusal() {
        assert!(is_untrusted("Workspace not trusted: /w/new"));
        assert!(!is_untrusted("acting is off"));
        assert!(is_off(&format!("forbidden: {OFF_MESSAGE}")));
    }

    #[test]
    fn a_held_question_is_answered_by_its_prompt_id_and_a_screen_one_by_its_row() {
        use notchling_engine::transcript::snapshot::{PendingQuestion, QuestionItem};
        let item = QuestionItem {
            header: "DB".into(),
            question: "Which db?".into(),
            options: vec!["Postgres".into(), "SQLite".into()],
            details: vec![],
            multi_select: false,
        };
        let held = PendingQuestion {
            tool_use_id: "hook:p7-19a".into(),
            items: vec![item.clone()],
        };
        let picked = [Answer::Options(vec![2])];
        assert_eq!(
            Request::answer_for("job-a", &held, &picked).args,
            json!({"id": "p7-19a", "answers": [[2]]})
        );
        // Read from the transcript or the screen: typed into the row's dialog, page by page.
        let typed = PendingQuestion {
            tool_use_id: "toolu_01".into(),
            items: vec![item],
        };
        assert_eq!(
            Request::answer_for("job-a", &typed, &picked).args,
            json!({"id": "job-a", "answers": [[2]], "question": {"items": [{"header": "DB",
                "question": "Which db?", "options": ["Postgres", "SQLite"], "details": [],
                "multiSelect": false}]}})
        );
    }

    #[test]
    fn requests_have_the_plan_shapes() {
        assert_eq!(
            Request::reply("s1", "go on"),
            Request {
                op: "session.reply",
                args: json!({"id": "s1", "text": "go on"})
            }
        );
        let answers = [
            Answer::Options(vec![2]),
            Answer::Options(vec![1, 3]),
            Answer::Text("neither".into()),
        ];
        assert_eq!(
            Request::answer("s1", &answers).args,
            json!({"id": "s1", "answers": [[2], [1, 3], "neither"]})
        );
        for (d, word) in [
            (Decision::Allow, "allow"),
            (Decision::Deny, "deny"),
            (Decision::Always, "always"),
        ] {
            let r = Request::permission("s1", d);
            assert_eq!(r.op, "session.permission");
            assert_eq!(r.args, json!({"id": "s1", "decision": word}));
        }
        assert_eq!(Request::interrupt("s1").op, "session.interrupt");
        assert_eq!(Request::stop("s1").op, "session.stop");
        assert_eq!(Request::remove("s1").op, "session.remove");
        assert_eq!(Request::take_back("s1").op, "session.take_back");
        assert_eq!(
            Request::new_session("/w/api", None, &[]).args,
            json!({"folder": "/w/api"})
        );
        assert_eq!(
            Request::new_session("/w/api", Some("/h/.claude-lanes/b"), &["--x".into()]).args,
            json!({"folder": "/w/api", "config_dir": "/h/.claude-lanes/b", "flags": ["--x"]})
        );
        assert_eq!(
            Request::trust_folder("/w/new"),
            Request {
                op: "folder.trust",
                args: json!({"folder": "/w/new"})
            }
        );
        assert_eq!(
            Request::move_session("s1", "/h/.claude-lanes/b").args,
            json!({"id": "s1", "to_config_dir": "/h/.claude-lanes/b"})
        );
        assert_eq!(
            Request::command("s1", "/compact").args,
            json!({"id": "s1", "command": "/compact"})
        );
    }
}
