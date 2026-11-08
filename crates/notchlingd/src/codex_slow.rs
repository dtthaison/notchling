//! The Codex jobs that can outlast a reply: starting a new session (Codex names it when it is
//! ready) and `codex delete`. A client gives up on a call after 5 s, so each runs on its own thread
//! ([`late_reply`]) and the reply waits [`REPLY_WAIT`]: a job done by then answers the call as
//! before, and a job still going is answered `starting`/`deleting` and reports its end as an event
//! on topic `tools`, so the call never fails while the job goes on.
//!
//! | Late end | Event |
//! | --- | --- |
//! | a new session is named | `codex.turn {id: "codex:<session>", session, state: "named", draft}` |
//! | a new session isn't started | `codex.turn {id: null, state: "not_started", draft, error}` |
//! | `codex delete` worked | `session.changed {id, session, row: null, deleted: true}` |
//! | `codex delete` failed | `codex.turn {id, state: "delete_failed", error}` |
//!
//! `draft` is the row id the caller sent as `draft` (`codex-new:<uuid>`): the app follows its draft
//! row to the session it became. Without one a new session's events are the ordinary ones.

use super::*;
use crate::late_reply;

/// The draft row ids a caller may name: the app's, `codex-new:<uuid>`.
const DRAFT_PREFIX: &str = "codex-new:";
const MAX_DRAFT_BYTES: usize = 80;

/// The reply for a message that was sent.
pub(super) fn sent_json(sent: Sent) -> Value {
    match sent {
        Sent::Started { session } => {
            json!({"id": row_id(&session), "session": session, "state": "started"})
        }
        Sent::Queued { session, waiting } => json!({
            "id": row_id(&session), "session": session, "state": "queued", "waiting": waiting
        }),
    }
}

pub(super) fn send_error(e: SendError) -> ApiError {
    match e {
        SendError::NotAllowed => forbidden(ACTING_OFF),
        other => ApiError::new(ErrorCode::Failed, other.to_string()),
    }
}

impl CodexOps {
    fn reply_wait(&self) -> Duration {
        Duration::from_millis(self.reply_wait_ms.load(Ordering::SeqCst))
    }

    /// `args.draft`: the row id of the draft a new session comes from, if the caller sent one.
    pub(super) fn draft_arg(args: &Value, new_session: bool) -> Result<Option<String>, ApiError> {
        if args.get("draft").is_none() {
            return Ok(None);
        }
        let draft = text_arg(args, "draft")?;
        if !new_session {
            return Err(bad("draft goes with draft_folder, not an existing session"));
        }
        if !draft.starts_with(DRAFT_PREFIX) || draft.len() > MAX_DRAFT_BYTES {
            return Err(bad("draft is a Codex draft row id (codex-new:<uuid>)"));
        }
        Ok(Some(draft.to_string()))
    }

    /// Starts a new session. The reply waits for Codex to name it, but only [`REPLY_WAIT`]; after
    /// that it is `starting` and the naming (or the failure) comes as an event carrying `draft`.
    pub(super) fn send_new(
        &self,
        request: Request,
        draft: Option<String>,
    ) -> Result<Value, ApiError> {
        let runs = self.runs.clone();
        let room = self.room.clone();
        let named = draft.clone();
        let answer = late_reply::run(
            self.reply_wait(),
            move || runs.send(request),
            move |late| announce_late(room.as_ref(), named.as_deref(), late),
            Err(SendError::NotStarted(
                "Couldn't start a thread to send the message".to_string(),
            )),
        );
        match answer {
            Some(sent) => sent.map(sent_json).map_err(send_error),
            None => Ok(json!({"state": "starting", "draft": draft})),
        }
    }

    /// Stops the session's turn, then runs `codex delete --force` on it. The reply waits
    /// [`REPLY_WAIT`]; a delete still going is `deleting` and ends as an event.
    pub(super) fn delete_session(
        &self,
        session: String,
        codex: PathBuf,
        path: String,
    ) -> Result<Value, ApiError> {
        let job = DeleteJob {
            runs: self.runs.clone(),
            launcher: self.launcher.clone(),
            room: self.room.clone(),
            codex_home: self.setup.codex_home.to_string_lossy().into_owned(),
            session: session.clone(),
            codex,
            path,
        };
        let room = self.room.clone();
        let failed = session.clone();
        let answer = late_reply::run(
            self.reply_wait(),
            move || job.run(),
            move |late| {
                if let Err(e) = late {
                    room.publish(
                        topics::TOOLS,
                        topics::CODEX_TURN,
                        &json!({"id": row_id(&failed), "session": failed,
                                "state": "delete_failed", "error": e.message}),
                    );
                }
            },
            Err(ApiError::new(
                ErrorCode::Failed,
                "Couldn't start a thread to delete the session",
            )),
        );
        match answer {
            Some(done) => done.map(|()| json!({"deleted": true})),
            None => Ok(json!({"deleted": false, "state": "deleting"})),
        }
    }
}

/// What a new session's late end tells the app (nothing without a draft to name).
fn announce_late(room: &dyn Room, draft: Option<&str>, late: Result<Sent, SendError>) {
    let Some(draft) = draft else {
        return;
    };
    let data = match late {
        Ok(Sent::Started { session } | Sent::Queued { session, .. }) => json!({
            "id": row_id(&session), "session": session, "state": "named", "draft": draft
        }),
        Err(e) => json!({
            "id": Value::Null, "state": "not_started", "draft": draft,
            "error": send_error(e).message
        }),
    };
    room.publish(topics::TOOLS, topics::CODEX_TURN, &data);
}

/// Everything a delete needs, owned, so it can run on its own thread.
struct DeleteJob {
    runs: CodexRuns,
    launcher: Arc<dyn Launcher>,
    room: Arc<dyn Room>,
    codex_home: String,
    session: String,
    codex: PathBuf,
    path: String,
}

impl DeleteJob {
    fn run(self) -> Result<(), ApiError> {
        // The turn is stopped and its waiting messages dropped first, and given a moment to let go
        // of the file `codex delete` removes.
        if self.runs.forget(&self.session) {
            let until = Instant::now() + STOP_SETTLE;
            while self.runs.is_running(&self.session) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let command = Command::from_args(
            &self.codex.to_string_lossy(),
            &["delete".into(), "--force".into(), self.session.clone()],
        )
        .with_env("PATH", Some(&self.path))
        .with_env("CODEX_HOME", Some(&self.codex_home));
        let done = self
            .launcher
            .run(&command, DELETE_TIMEOUT)
            .map_err(|e| ApiError::new(ErrorCode::Failed, e.to_string()))?;
        if done.code != 0 {
            let line = done.stderr.lines().rev().find(|l| !l.trim().is_empty());
            return Err(ApiError::new(
                ErrorCode::Failed,
                line.map_or_else(
                    || format!("codex delete exited with code {}.", done.code),
                    |l| l.trim().to_string(),
                ),
            ));
        }
        self.room.publish(
            topics::TOOLS,
            topics::SESSION_CHANGED,
            &json!({"id": row_id(&self.session), "session": self.session,
                    "row": Value::Null, "deleted": true}),
        );
        Ok(())
    }
}
