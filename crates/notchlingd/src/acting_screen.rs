use super::*;
use crate::acting_events::NOT_AT_A_PROMPT;
use notchling_engine::outbox::Seen;
use notchling_engine::reply::read::{dialog_up, read_dialog};
use notchling_engine::terminal::screen_dialog::{DialogPick, ScreenDialog};

const SHELL_COMMAND_MAX: usize = 64 * 1024;

pub(super) struct AtPrompt<'a> {
    pub(super) inner: &'a dyn ScreenCheck,
    pub(super) still: &'a dyn Fn() -> bool,
}

impl ScreenCheck for AtPrompt<'_> {
    fn dialog_open(&self, lines: &[String]) -> bool {
        self.inner.dialog_open(lines)
    }
    fn shows(&self, question: &str, lines: &[String]) -> bool {
        self.inner.shows(question, lines)
    }
    fn is_review(&self, lines: &[String]) -> bool {
        self.inner.is_review(lines)
    }
    fn at_prompt(&self) -> bool {
        self.inner.at_prompt() && (self.still)()
    }
}

fn dialog_json(dialog: &ScreenDialog) -> Value {
    let options: Vec<Value> = dialog
        .options
        .iter()
        .map(|o| json!({"number": o.number, "label": o.label, "selected": o.selected}))
        .collect();
    json!({"title": dialog.title, "body": dialog.body, "options": options})
}

impl ActingOps {
    pub(super) fn shell_output(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if !self.acting.is_on() {
            return Err(forbidden(ACTING_OFF));
        }
        if caller.tier != Tier::App {
            return Err(forbidden("reading a session's screen is for the app only"));
        }
        let command = text_arg(args, "command")?;
        if command.trim().is_empty() || command.len() > SHELL_COMMAND_MAX {
            return Err(bad("command is the shell command the session is running"));
        }
        let agent = self.agent(args)?;
        let no_terminal = || bad("this session has no terminal to read");
        if !matches!(&agent.target, AgentTarget::Job(_) if agent.can_reply()) {
            return Err(no_terminal());
        }
        let read = self.try_locked(&agent, |agent| {
            let job = match &agent.target {
                AgentTarget::Job(now) if agent.can_reply() => now.clone(),
                _ => return Err(no_terminal()),
            };
            if !self.running_now(&job) {
                return Ok(None);
            }
            let config_dir =
                config_dir_for_job(&self.roots, &job).or_else(|| agent.config_dir.clone());
            let mut pty = self.attacher.attach(&job, config_dir.as_deref())?;
            let lines = read_shell_output(pty.as_mut(), command);
            Ok(Some((lines, pty)))
        });
        match read {
            None => Ok(json!({"lines": [], "busy": true})),
            Some(Err(e)) => Err(e),
            Some(Ok(None)) => Ok(json!({"lines": []})),
            Some(Ok(Some((lines, pty)))) => {
                drop(pty);
                Ok(json!({ "lines": lines }))
            }
        }
    }

    fn running_now(&self, job: &str) -> bool {
        matches!(self.job_status(job).as_deref(), Some("busy" | "shell"))
    }

    pub(super) fn dialog(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if !self.acting.is_on() {
            return Err(forbidden(ACTING_OFF));
        }
        if caller.tier != Tier::App {
            return Err(forbidden("reading a session's screen is for the app only"));
        }
        let agent = self.agent(args)?;
        let no_terminal = || bad("this session has no terminal to read");
        if !matches!(&agent.target, AgentTarget::Job(_) if agent.can_reply()) {
            return Err(no_terminal());
        }
        let read = self.try_locked(&agent, |agent| {
            let job = match &agent.target {
                AgentTarget::Job(now) if agent.can_reply() => now.clone(),
                _ => return Err(no_terminal()),
            };
            let config_dir =
                config_dir_for_job(&self.roots, &job).or_else(|| agent.config_dir.clone());
            let mut pty = self.attacher.attach(&job, config_dir.as_deref())?;
            let dialog = read_dialog(pty.as_mut())
                .map_err(|f| ApiError::new(ErrorCode::Failed, f.to_string()))?;
            Ok((dialog, pty))
        });
        match read {
            None => Ok(json!({"dialog": null, "busy": true})),
            Some(Err(e)) => Err(e),
            Some(Ok((dialog, pty))) => {
                drop(pty);
                Ok(json!({"dialog": dialog.as_ref().map(dialog_json)}))
            }
        }
    }

    pub(crate) fn dialog_seen(&self, agent: &Agent) -> Seen {
        if !self.acting.is_on() {
            return Seen::Unknown;
        }
        let probe = self.try_locked(agent, |agent| -> Result<_, ApiError> {
            let AgentTarget::Job(job) = &agent.target else {
                return Ok(None);
            };
            let config_dir =
                config_dir_for_job(&self.roots, job).or_else(|| agent.config_dir.clone());
            let mut pty = self.attacher.attach(job, config_dir.as_deref())?;
            let up = dialog_up(pty.as_mut());
            Ok(Some((up, pty)))
        });
        match probe {
            Some(Ok(Some((up, pty)))) => {
                drop(pty);
                match up {
                    true => Seen::Open,
                    false => Seen::Closed,
                }
            }
            _ => Seen::Unknown,
        }
    }

    pub(super) fn dialog_pick(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if caller.tier != Tier::App {
            return Err(forbidden(
                "answering a session's dialog is for the app only",
            ));
        }
        let who = who(caller, args);
        let number = args
            .get("number")
            .and_then(Value::as_u64)
            .ok_or_else(|| bad("number is the option to press"))?;
        if !(1..=9).contains(&number) {
            return Err(bad("number is one of the options, 1 to 9"));
        }
        let pick = DialogPick {
            title: text_arg(args, "title")?.to_owned(),
            number,
            label: text_arg(args, "label")?.to_owned(),
        };
        let agent = self.agent(args)?;
        self.locked(&agent, |agent| {
            self.play_locked(agent, &plan::dialog(&pick), &mut NoLog, None)
        })?;
        self.note_answered(
            session_ref(&agent),
            PromptKind::Question,
            None,
            AnswerKind::Picked,
            &who,
            None,
        );
        Ok(json!({"answered": "terminal", "number": number}))
    }

    pub(super) fn dialog_dismiss(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if caller.tier != Tier::App {
            return Err(forbidden(
                "dismissing a session's dialog is for the app only",
            ));
        }
        let first = self.agent(args)?;
        self.locked(&first, |agent| {
            let AgentTarget::Job(job) = &agent.target else {
                return Err(bad("this session has no terminal to read").into());
            };
            let at_prompt = || matches!(self.job_status(job).as_deref(), Some("waiting" | "idle"));
            if !at_prompt() {
                return Err(ApiError::new(ErrorCode::Failed, NOT_AT_A_PROMPT).into());
            }
            self.play_inner(agent, &plan::dismiss(), &mut NoLog, None, Some(&at_prompt))
        })?;
        Ok(json!({"dismissed": true}))
    }
}
