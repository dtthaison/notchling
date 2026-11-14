use crate::acting_ops::{now_ms, ActingOps};
use crate::late_emit::Events;
use notchling_engine::reply::exec::Failure;
use notchling_engine::sessions::lanes::lane_at_path;
use notchling_engine::sessions::Agent;
use notchling_events::envelope::ActorTier;
use notchling_events::field::{Count, Millis, Tool};
use notchling_events::kinds::agents::{
    AnswerKind, AnswerSurface, ControlAction, HeldThen, PromptKind, ReplyVia, SendResult,
    SessionAnswered, SessionControl, SessionReply, SessionReplyHeld,
};
use notchling_events::kinds::choices::{DoneOrFailed, FailWhy, SwitchName};
use notchling_events::kinds::security::{SwitchCause, SwitchChanged};
use notchling_events::spec::Outcome;
use notchling_events::{scope, Account, Device, Event, Folder, Ref, Session};
use notchling_local_api::{ApiError, Caller, ErrorCode, Tier};
use serde_json::Value;

pub(crate) const LOG_DOWN: &str = "the audit log can't be written";

pub(crate) struct PlayError {
    pub(crate) err: ApiError,
    pub(crate) why: FailWhy,
}

impl From<ApiError> for PlayError {
    fn from(err: ApiError) -> PlayError {
        PlayError {
            err,
            why: FailWhy::Other,
        }
    }
}

impl From<PlayError> for ApiError {
    fn from(failure: PlayError) -> ApiError {
        failure.err
    }
}

pub(crate) fn why_of(failure: &Failure) -> FailWhy {
    match failure {
        Failure::Pty(_) => FailWhy::Pty,
        Failure::Timeout => FailWhy::Timeout,
        Failure::Dialog => FailWhy::DialogOpen,
        Failure::QuestionGone => FailWhy::QuestionGone,
        Failure::QuestionMismatch => FailWhy::QuestionMismatch,
        Failure::NotPermission => FailWhy::NotPermission,
        Failure::PermissionMismatch => FailWhy::PermissionMismatch,
        Failure::NoAlways => FailWhy::NoAlways,
        Failure::PermissionStillOpen => FailWhy::PermissionStillOpen,
        Failure::NoInput => FailWhy::NoInput,
        Failure::NotCleared => FailWhy::NotCleared,
        Failure::NotTaken => FailWhy::NotTaken,
        Failure::NotSent => FailWhy::NotSent,
    }
}

pub(crate) fn attach_why(err: &ApiError) -> FailWhy {
    match err.code {
        ErrorCode::Unimplemented => FailWhy::NoTerminal,
        _ => FailWhy::AttachFailed,
    }
}

pub(crate) struct Who {
    pub(crate) via: ReplyVia,
    pub(crate) surface: AnswerSurface,
    pub(crate) device: Option<Ref<Device>>,
}

pub(crate) fn who(caller: &Caller, args: &Value) -> Who {
    let actor = scope::current().actor;
    let phone = actor.as_ref().is_some_and(|a| a.tier == ActorTier::Phone);
    let device = actor.and_then(|a| a.device).filter(|_| phone);
    let notch = args.get("from").and_then(Value::as_str) == Some("notch");
    let via = match (caller.tier == Tier::Agent, phone, notch) {
        (true, _, _) => ReplyVia::Agent,
        (_, true, _) => ReplyVia::Phone,
        (_, _, true) => ReplyVia::Notch,
        _ => ReplyVia::Chat,
    };
    let surface = match via {
        ReplyVia::Phone => AnswerSurface::Phone,
        ReplyVia::Notch => AnswerSurface::Notch,
        _ => AnswerSurface::Chat,
    };
    Who {
        via,
        surface,
        device,
    }
}

pub(crate) fn session_ref(agent: &Agent) -> Ref<Session> {
    Ref::new(&agent.session_id().unwrap_or_else(|| agent.id.clone()))
}

pub(crate) struct Target {
    pub(crate) action: ControlAction,
    pub(crate) command: Option<String>,
    pub(crate) session: Option<Ref<Session>>,
    pub(crate) folder: Option<String>,
    pub(crate) account: Option<String>,
}

fn word_of(command: &str) -> Option<String> {
    let word: String = command
        .trim()
        .trim_start_matches('/')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
        .collect();
    (!word.is_empty()).then_some(word)
}

impl ActingOps {
    pub fn with_events(mut self, events: Events) -> ActingOps {
        self.events = Some(events);
        self
    }

    fn account_of(&self, dir: Option<&str>) -> Option<String> {
        dir.and_then(|d| lane_at_path(&self.roots, d))
            .map(|lane| lane.account_id)
            .filter(|id| !id.is_empty())
    }

    pub(crate) fn target_of(&self, op: &str, args: &Value) -> Option<Target> {
        use notchling_local_api::ops;
        let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_owned);
        let session = || self.agent(args).ok().map(|a| session_ref(&a));
        let (action, command, session, folder, account) = match op {
            ops::SESSION_STOP => (ControlAction::Stop, None, session(), None, None),
            ops::SESSION_REMOVE => (ControlAction::Remove, None, session(), None, None),
            ops::SESSION_INTERRUPT => (ControlAction::Interrupt, None, session(), None, None),
            ops::SESSION_TAKE_BACK => (ControlAction::TakeBack, None, session(), None, None),
            ops::SESSION_NEW => (
                ControlAction::New,
                None,
                None,
                text("folder"),
                self.account_of(text("config_dir").as_deref()),
            ),
            ops::FOLDER_TRUST => (
                ControlAction::TrustFolder,
                None,
                None,
                text("folder"),
                self.account_of(text("config_dir").as_deref()),
            ),
            ops::SESSION_MOVE => (
                ControlAction::Move,
                None,
                session(),
                None,
                self.account_of(text("to_config_dir").as_deref()),
            ),
            ops::SESSION_COMMAND => (
                ControlAction::SlashCommand,
                text("command").as_deref().and_then(word_of),
                session(),
                None,
                None,
            ),
            _ => return None,
        };
        Some(Target {
            action,
            command,
            session,
            folder,
            account,
        })
    }

    pub(crate) fn note_control(&self, target: &Target, result: DoneOrFailed, commit: bool) -> bool {
        let Some(events) = &self.events else {
            return true;
        };
        let event = Event::new(
            &SessionControl {
                action: target.action,
                command: target.command.as_deref().map(Tool::new),
                session: target.session.clone(),
                folder: target.folder.as_deref().map(Ref::<Folder>::new),
                account: target.account.as_deref().map(Ref::<Account>::new),
                result,
            },
            match result {
                DoneOrFailed::Done => Outcome::Ok,
                _ => Outcome::Failed,
            },
            now_ms(),
        );
        scope::mark_recorded();
        send(events, event, commit)
    }

    pub(crate) fn note_reply(
        &self,
        session: Ref<Session>,
        via: ReplyVia,
        chars: usize,
        failure: Option<FailWhy>,
        waited_ms: Option<u64>,
        commit: bool,
    ) -> bool {
        let Some(events) = &self.events else {
            return true;
        };
        let (result, outcome) = match failure {
            None => (SendResult::Sent, Outcome::Ok),
            Some(_) => (SendResult::Failed, Outcome::Failed),
        };
        let event = Event::new(
            &SessionReply {
                session,
                via,
                chars: Count(chars as u64),
                images: Count(0),
                result,
                why: failure,
                waited_ms: waited_ms.map(Millis),
            },
            outcome,
            now_ms(),
        );
        scope::mark_recorded();
        send(events, event, commit)
    }

    pub(crate) fn note_acting_off(&self, args: &Value, via: ReplyVia) {
        let session = self
            .agent(args)
            .map_or_else(|_| Ref::<Session>::new("unknown"), |a| session_ref(&a));
        let chars = args
            .get("text")
            .and_then(Value::as_str)
            .map_or(0, |t| t.chars().count());
        self.note_reply(session, via, chars, Some(FailWhy::ActingOff), None, false);
    }

    pub(crate) fn note_held(&self, session: Ref<Session>, then: HeldThen) {
        if let Some(events) = &self.events {
            events.push(Event::new(
                &SessionReplyHeld { session, then },
                Outcome::Info,
                now_ms(),
            ));
        }
    }

    pub(crate) fn note_answered(
        &self,
        session: Ref<Session>,
        prompt: PromptKind,
        tool: Option<&str>,
        answer: AnswerKind,
        who: &Who,
        waited_ms: Option<u64>,
    ) {
        if let Some(events) = &self.events {
            events.push(Event::new(
                &SessionAnswered {
                    session,
                    prompt,
                    tool: tool.map(Tool::new),
                    answer,
                    by: who.surface,
                    device: who.device.clone(),
                    waited_ms: waited_ms.map(Millis),
                },
                Outcome::Ok,
                now_ms(),
            ));
            scope::mark_recorded();
        }
    }

    pub(crate) fn note_acting(&self, on: bool, args: &Value) {
        if let Some(events) = &self.events {
            let cause = match args.get("restored").and_then(Value::as_bool) {
                Some(true) => SwitchCause::RestoredAtLaunch,
                _ => SwitchCause::User,
            };
            events.push(Event::new(
                &SwitchChanged {
                    switch: SwitchName::Acting,
                    on,
                    cause,
                },
                Outcome::Ok,
                now_ms(),
            ));
            scope::mark_recorded();
        }
    }
}

fn send(events: &Events, event: Event, commit: bool) -> bool {
    if commit {
        events.commit(event)
    } else {
        events.push(event);
        true
    }
}

pub(crate) fn log_down() -> ApiError {
    ApiError::new(ErrorCode::Failed, LOG_DOWN)
}
