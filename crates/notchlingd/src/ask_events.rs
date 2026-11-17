use crate::late_emit::Events;
use notchling_engine::ask::{Asker, By, Mode, Row};
use notchling_events::field::{Count, Millis};
use notchling_events::kinds::agents::SessionAsk;
use notchling_events::kinds::choices::{Answerer, AskMode, AskThen, AskerKind};
use notchling_events::spec::Outcome;
use notchling_events::{scope, Event, Ref, Session};

pub(crate) struct Facts {
    pub session: Ref<Session>,
    pub then: AskThen,
    pub asker: Asker,
    pub by: Option<By>,
    pub mode: Mode,
    pub chars: usize,
    pub choices: usize,
    pub answer_chars: usize,
    pub waited_ms: i64,
}

impl Facts {
    pub(crate) fn of(row: &Row, session: Ref<Session>, then: AskThen, now_ms: i64) -> Facts {
        Facts {
            session,
            then,
            asker: row.asker,
            by: row.by,
            mode: row.mode,
            chars: row.question.chars().count(),
            choices: row.choices.as_ref().map_or(0, Vec::len),
            answer_chars: row.answer.as_ref().map_or(0, |a| a.chars().count()),
            waited_ms: now_ms.saturating_sub(row.created_ms),
        }
    }

    pub(crate) fn refused(
        session: Ref<Session>,
        mode: Mode,
        chars: usize,
        choices: usize,
    ) -> Facts {
        Facts {
            session,
            then: AskThen::Refused,
            asker: Asker::Unknown,
            by: None,
            mode,
            chars,
            choices,
            answer_chars: 0,
            waited_ms: 0,
        }
    }
}

fn asker_kind(asker: Asker) -> AskerKind {
    match asker {
        Asker::Main => AskerKind::Main,
        Asker::Subagent => AskerKind::Subagent,
        Asker::Codex => AskerKind::Codex,
        Asker::Antigravity => AskerKind::Antigravity,
        Asker::Unknown => AskerKind::Unknown,
    }
}

fn answerer(by: Option<By>) -> Answerer {
    match by {
        Some(By::Mac) => Answerer::Mac,
        Some(By::Phone) => Answerer::Phone,
        Some(By::Main) => Answerer::Main,
        None => Answerer::NoOne,
    }
}

pub(crate) fn emit(events: Option<&Events>, facts: Facts, now_ms: i64) {
    let Some(events) = events else {
        return;
    };
    let outcome = if facts.then == AskThen::Refused {
        Outcome::Refused
    } else {
        Outcome::Info
    };
    events.push(Event::new(
        &SessionAsk {
            session: facts.session,
            then: facts.then,
            asker: asker_kind(facts.asker),
            by: answerer(facts.by),
            mode: match facts.mode {
                Mode::Background => AskMode::Background,
                Mode::Wait => AskMode::Wait,
            },
            chars: Count(facts.chars as u64),
            choices: Count(facts.choices as u64),
            answer_chars: Count(facts.answer_chars as u64),
            waited: Millis(u64::try_from(facts.waited_ms).unwrap_or(0)),
        },
        outcome,
        now_ms,
    ));
    scope::mark_recorded();
}
