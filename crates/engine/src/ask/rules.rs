use super::model::{
    AskState, Asker, OpenError, Row, MAX_ANSWER_BYTES, MAX_CHOICES, MAX_CHOICE_CHARS,
    MAX_QUESTION_CHARS, MIN_CHOICES,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who<'a> {
    Person,
    Agent { session: &'a str, is_main: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    NotOpen(AskState),
    PersonOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerError {
    Missing,
    TooLong,
    NoSuchChoice,
}

fn agents_may_be_asked(asker: Asker) -> bool {
    matches!(asker, Asker::Subagent | Asker::Codex | Asker::Antigravity)
}

pub fn main_may_see(row: &Row, session: &str) -> bool {
    row.state == AskState::Open
        && row.main_may_answer
        && agents_may_be_asked(row.asker)
        && row.session == session
}

pub fn may_answer(row: &Row, who: &Who) -> Result<(), Refused> {
    if row.state != AskState::Open {
        return Err(Refused::NotOpen(row.state));
    }
    match who {
        Who::Person => Ok(()),
        Who::Agent { session, is_main } => {
            if main_may_see(row, session) && *is_main {
                Ok(())
            } else {
                Err(Refused::PersonOnly)
            }
        }
    }
}

pub fn check_new(
    question: &str,
    choices: Option<&[String]>,
    on_worker: bool,
) -> Result<(), OpenError> {
    if question.trim().is_empty() {
        return Err(OpenError::Empty);
    }
    if question.chars().count() > MAX_QUESTION_CHARS {
        return Err(OpenError::TooLong);
    }
    match choices {
        Some(all) => {
            if !(MIN_CHOICES..=MAX_CHOICES).contains(&all.len()) {
                return Err(OpenError::Choices);
            }
            if all
                .iter()
                .any(|c| c.trim().is_empty() || c.chars().count() > MAX_CHOICE_CHARS)
            {
                return Err(OpenError::ChoiceText);
            }
        }
        None if on_worker => return Err(OpenError::WorkerNeedsChoices),
        None => {}
    }
    Ok(())
}

pub fn answer_text(
    row: &Row,
    text: Option<&str>,
    choice: Option<u8>,
) -> Result<(String, Option<u8>), AnswerError> {
    match (text, choice) {
        (Some(text), None) => {
            if text.trim().is_empty() {
                return Err(AnswerError::Missing);
            }
            if text.len() > MAX_ANSWER_BYTES {
                return Err(AnswerError::TooLong);
            }
            Ok((text.to_string(), None))
        }
        (None, Some(n)) => {
            let at = usize::from(n).checked_sub(1);
            at.and_then(|at| row.choices.as_ref()?.get(at))
                .map(|text| (text.clone(), Some(n)))
                .ok_or(AnswerError::NoSuchChoice)
        }
        _ => Err(AnswerError::Missing),
    }
}

pub fn cut(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    if max_chars > 0 {
        out.push('…');
    }
    out
}
