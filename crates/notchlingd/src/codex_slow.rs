use super::*;
use crate::late_reply;
use crate::outbox_ops::SendArgs;
use crate::outbox_runners::Delivery;
use notchling_engine::outbox::Via;

const DRAFT_PREFIX: &str = "codex-new:";
const MAX_DRAFT_BYTES: usize = 80;

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

fn options_of(pick: &Pick) -> Option<Value> {
    let mut options = serde_json::Map::new();
    if let Some(model) = &pick.model {
        options.insert("model".into(), json!(model));
    }
    if let Some(effort) = &pick.effort {
        options.insert("effort".into(), json!(effort));
    }
    (!options.is_empty()).then_some(Value::Object(options))
}

fn failed_with(e: ApiError) -> Delivery {
    Delivery::Failed(e.message)
}

impl CodexOps {
    pub(crate) fn attach_outbox(&self, outbox: Arc<OutboxOps>) {
        let _ = self.outbox.set(outbox);
    }

    pub(crate) fn check_session(&self, session: &str) -> Result<(), ApiError> {
        if self.runs.is_running(session) || find_rollout(&self.sessions_dir(), session).is_some() {
            return Ok(());
        }
        Err(bad("No such session."))
    }

    pub(crate) fn is_busy(&self, session: &str) -> bool {
        self.runs.is_running(session)
    }

    pub(super) fn enqueue(
        &self,
        outbox: &OutboxOps,
        session: &str,
        text: &str,
        pick: &Pick,
    ) -> Result<Value, ApiError> {
        let row = outbox.send(&SendArgs {
            id: row_id(session),
            text: text.to_string(),
            clear_box: false,
            options: options_of(pick),
            via: Via::Chat,
        })?;
        Ok(json!({
            "id": row_id(session),
            "session": session,
            "state": "queued",
            "waiting": outbox.position(&row),
            "outboxId": row.id,
        }))
    }

    pub(crate) fn start_turn(
        &self,
        session: &str,
        text: &str,
        options: Option<&Value>,
    ) -> Result<(), Delivery> {
        self.fenced(session).map_err(failed_with)?;
        let pick = self
            .pick(options.unwrap_or(&Value::Null))
            .map_err(failed_with)?;
        let folder = self.folder_of(session).map_err(failed_with)?;
        let (codex, path) = (self.resolve)().ok_or_else(|| {
            Delivery::Failed("Couldn't find the codex command. Install the Codex CLI first.".into())
        })?;
        self.runs.set_command(codex, Some(path));
        self.runs.set_network(self.network_allowed());
        let request = Request {
            text: text.to_string(),
            session: Some(session.to_string()),
            folder,
            pick,
        };
        match self.runs.send_if_idle(request) {
            Ok(_) => Ok(()),
            Err(SendError::Busy | SendError::NotAllowed) => Err(Delivery::Busy),
            Err(SendError::Cut) => Err(Delivery::Cut),
            Err(other) => Err(Delivery::Failed(other.to_string())),
        }
    }

    fn reply_wait(&self) -> Duration {
        Duration::from_millis(self.reply_wait_ms.load(Ordering::SeqCst))
    }

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

    pub(super) fn job_arg(args: &Value) -> Result<Option<String>, ApiError> {
        match args.get("job") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(job)) if !job.is_empty() && job.len() <= MAX_DRAFT_BYTES => {
                Ok(Some(job.clone()))
            }
            Some(_) => Err(bad("job is the id of a cluster job")),
        }
    }

    pub(super) fn send_new(
        &self,
        request: Request,
        draft: Option<String>,
        job: Option<String>,
    ) -> Result<Value, ApiError> {
        let runs = self.runs.clone();
        let room = self.room.clone();
        let named = draft.clone();
        let link = self.job_link.get().cloned().zip(job);
        let answer = late_reply::run(
            self.reply_wait(),
            move || runs.send(request),
            move |late| {
                if let Some((link, job)) = &link {
                    match &late {
                        Ok(Sent::Started { session } | Sent::Queued { session, .. }) => {
                            link.session_started(job, &row_id(session))
                        }
                        Err(e) => link.start_failed(job, &send_error(e.clone()).message),
                    }
                }
                announce_late(room.as_ref(), named.as_deref(), late)
            },
            Err(SendError::NotStarted(
                "Couldn't start a thread to send the message".to_string(),
            )),
        );
        match answer {
            Some(sent) => sent.map(sent_json).map_err(send_error),
            None => Ok(json!({"state": "starting", "draft": draft})),
        }
    }

    pub(super) fn delete_session(
        &self,
        session: String,
        codex: PathBuf,
        path: String,
    ) -> Result<Value, ApiError> {
        let job = DeleteJob {
            outbox: self.outbox.get().cloned(),
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

struct DeleteJob {
    outbox: Option<Arc<crate::outbox_ops::OutboxOps>>,
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
        if let Some(outbox) = &self.outbox {
            outbox.remove_session(&row_id(&self.session));
        }
        Ok(())
    }
}
