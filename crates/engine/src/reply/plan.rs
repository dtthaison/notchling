//! The planners: what is typed into a session's attach terminal, step by step, with the pauses and
//! screen checks between. Pure: a [`Plan`] is data, played by [`super::exec::execute`].
//!
//! Each follows its Swift twin in `ReplySender` (attach, wait, check, keys, detach). Where this adds
//! a check the Swift code lacks, it only ever stops typing, never changes a key or a pause.

use super::keys::{self, timing::*, ENTER, ESC, SUBMIT, UP};
use super::question::{page_keys, Choice, PendingQuestion};
use super::shell;
pub use crate::terminal::permission_dialog::{self, Decision, Expected};

/// One thing the executor does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Read output until none comes for `quiet_ms`, for at most `timeout_ms`. `required`: when
    /// nothing at all was drawn by then the session didn't open, and the plan stops (`Timeout`).
    WaitQuiet {
        quiet_ms: u64,
        timeout_ms: u64,
        required: bool,
    },
    /// Stop with `Dialog` when a question or permission dialog is open: typed text would answer it.
    RefuseDialog,
    /// Stop with `QuestionGone` unless a dialog is open.
    RequireDialog,
    /// Keys as they are (control keys, digits).
    Keys(String),
    /// Text as one bracketed paste, with what could end the paste taken out ([`keys::bracketed`]).
    Paste(String),
    Pause(u64),
    /// Press Left until the dialog shows `question`; after `tries` presses without it, `QuestionMismatch`.
    WalkBackTo {
        question: String,
        tries: usize,
    },
    /// Stop with `QuestionMismatch` unless the dialog shows `question`.
    ExpectQuestion(String),
    /// Read until `until` holds, for at most `timeout_ms`; `QuestionMismatch` when it never does.
    WaitFor {
        until: Until,
        timeout_ms: u64,
    },
    /// On the review screen: the submit key, then wait (at most `timeout_ms`) for the dialog to close.
    SubmitIfReview {
        timeout_ms: u64,
    },
    /// The moment the take-back's Up goes: `popAll` lines count from a little before it.
    MarkQueue,
    /// Look for the transcript's `popAll` of `text` for at most `timeout_ms`.
    AwaitPopAll {
        text: String,
        timeout_ms: u64,
    },
    /// End the steps here (still detaching) unless `AwaitPopAll` saw it.
    EndUnlessTaken,
    /// Read the permission dialog on screen ([`permission_dialog::read`]) and press the digit of
    /// the option that gives `decision`, and nothing else. Stops with nothing typed unless the
    /// screen shows a permission dialog (`NotPermission`) asking about `expect`
    /// (`PermissionMismatch`) with an option for `decision` (`NoAlways`, or `NotPermission` for a
    /// dialog with no Yes or No).
    PressPermission {
        decision: Decision,
        expect: Expected,
    },
}

/// What [`Step::WaitFor`] waits for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Until {
    /// The dialog shows this question.
    Shows(String),
    /// The review screen, or no dialog any more.
    ReviewOrGone,
    /// The permission dialog [`Step::PressPermission`] answered isn't on screen any more
    /// (`PermissionStillOpen` when it stays).
    PermissionGone,
}

/// A whole attach: `steps`, then `detach` when they ended well (or at `EndUnlessTaken`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub detach: Vec<Step>,
}

impl Plan {
    /// Nothing to type: the caller doesn't attach at all.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Every write in order, as the bytes go out (a paste with its brackets).
    pub fn writes(&self) -> Vec<String> {
        self.steps
            .iter()
            .chain(&self.detach)
            .filter_map(|s| match s {
                Step::Keys(k) => Some(k.clone()),
                Step::Paste(t) => Some(keys::bracketed(t)),
                Step::SubmitIfReview { .. } => Some(SUBMIT.to_string()),
                _ => None,
            })
            .collect()
    }
}

/// Why a plan can't be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// Esc on an idle session opens the rewind menu; it is only for a working one.
    NotWorking,
}

fn attach(settle: Option<u64>) -> Step {
    match settle {
        Some(settle) => Step::WaitQuiet {
            quiet_ms: ATTACH_QUIET_MS,
            timeout_ms: settle + SETTLE_SLACK_MS,
            required: false,
        },
        None => Step::WaitQuiet {
            quiet_ms: ATTACH_QUIET_MS,
            timeout_ms: ATTACH_TIMEOUT_MS,
            required: true,
        },
    }
}

fn detach() -> Vec<Step> {
    vec![Step::WaitQuiet {
        quiet_ms: DETACH_QUIET_MS,
        timeout_ms: DETACH_TIMEOUT_MS,
        required: false,
    }]
}

/// `ReplySender.send(_:toJob:)`: wait for the screen, refuse a dialog, then the clear keys, the
/// paste and Enter with a pause after each. A `!` command is put back together first. The dialog is
/// looked for again just before the paste: one that opened after the clear keys would take the
/// text as its answer.
pub fn reply(text: &str) -> Plan {
    let text = shell::normalize(text);
    Plan {
        steps: vec![
            attach(None),
            Step::RefuseDialog,
            Step::Keys(keys::clear_input()),
            Step::Pause(KEY_PAUSE_MS),
            Step::RefuseDialog,
            Step::Paste(text),
            Step::Pause(KEY_PAUSE_MS),
            Step::Keys(ENTER.into()),
            Step::Pause(KEY_PAUSE_MS),
        ],
        detach: detach(),
    }
}

/// `ReplySender.interrupt`: Esc, only while the session works, after a short settle (a busy screen
/// never goes quiet).
pub fn interrupt(working: bool) -> Result<Plan, PlanError> {
    if !working {
        return Err(PlanError::NotWorking);
    }
    Ok(Plan {
        steps: vec![
            attach(Some(SETTLE_MS)),
            Step::Keys(ESC.into()),
            Step::Pause(KEY_PAUSE_MS),
        ],
        detach: detach(),
    })
}

/// `ReplySender.takeBackQueued`: clear the box, Up (which pulls the whole queue into it and logs a
/// `popAll` per message), clear it again, then queue `keep` (the older ones) again in order. Whether
/// `latest` was taken is read from the transcript, never the screen: Up on an empty box with no
/// queue brings back the last prompt, which may be the same words.
pub fn take_back(latest: &str, keep: &[String]) -> Plan {
    let mut steps = vec![
        attach(Some(SETTLE_MS)),
        Step::RefuseDialog,
        Step::Keys(keys::clear_input()),
        Step::Pause(TAKE_BACK_PAUSE_MS),
        Step::MarkQueue,
        Step::Keys(UP.into()),
        Step::AwaitPopAll {
            text: latest.to_string(),
            timeout_ms: POP_ALL_WAIT_MS,
        },
        Step::Keys(keys::clear_input()),
        Step::Pause(TAKE_BACK_PAUSE_MS),
        Step::EndUnlessTaken,
    ];
    for text in keep {
        steps.extend([
            Step::Paste(text.clone()),
            Step::Pause(TAKE_BACK_PAUSE_MS),
            Step::Keys(ENTER.into()),
            Step::Pause(TAKE_BACK_PAUSE_MS),
        ]);
    }
    Plan {
        steps,
        detach: detach(),
    }
}

/// `ReplySender.answer(_:to:toJob:)`: the dialog must be open; walk back to its first question if
/// the tabs were moved; then per question check the page shows it, type its page keys, and wait
/// for the next question (or the review screen, or the dialog gone) before going on; on the review
/// screen `1` submits. A page that shows anything else stops the typing (`QuestionMismatch`).
pub fn answer(choices: &[Choice], question: &PendingQuestion) -> Plan {
    let items = &question.items;
    let Some(first) = items.first() else {
        return Plan::default();
    };
    let mut steps = vec![
        attach(None),
        Step::RequireDialog,
        Step::WalkBackTo {
            question: first.question.clone(),
            tries: items.len() + 1,
        },
    ];
    for (index, (item, choice)) in items.iter().zip(choices).enumerate() {
        steps.push(Step::ExpectQuestion(item.question.clone()));
        for key in page_keys(choice, item) {
            steps.push(Step::Keys(key));
            steps.push(Step::Pause(KEY_PAUSE_MS));
        }
        let until = match items.get(index + 1) {
            Some(next) => Until::Shows(next.question.clone()),
            None => Until::ReviewOrGone,
        };
        steps.push(Step::WaitFor {
            until,
            timeout_ms: PAGE_WAIT_MS,
        });
    }
    steps.push(Step::SubmitIfReview {
        timeout_ms: SUBMIT_WAIT_MS,
    });
    Plan {
        steps,
        detach: detach(),
    }
}

/// Answers a background job's permission dialog that no hook holds: wait for the screen, read the
/// dialog, check it asks about what Chat showed, press the one digit of the chosen option, then
/// wait for the dialog to go. The digit is the only key: no Enter (a digit answers on its own),
/// and never Esc or Ctrl+C, which would interrupt or cancel on a screen this doesn't know.
pub fn permission(decision: Decision, expect: &Expected) -> Plan {
    Plan {
        steps: vec![
            attach(None),
            Step::PressPermission {
                decision,
                expect: expect.clone(),
            },
            Step::Pause(KEY_PAUSE_MS),
            Step::WaitFor {
                until: Until::PermissionGone,
                timeout_ms: PERMISSION_WAIT_MS,
            },
        ],
        detach: detach(),
    }
}
