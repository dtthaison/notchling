use super::keys::{self, timing::*, ENTER, ESC, SUBMIT, UP};
use super::question::{page_keys, Choice, PendingQuestion};
use super::shell;
pub use crate::terminal::permission_dialog::{self, Decision, Expected};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    WaitQuiet {
        quiet_ms: u64,
        timeout_ms: u64,
        required: bool,
    },
    RefuseDialog,
    RequireDialog,
    AwaitInput {
        timeout_ms: u64,
    },
    AwaitEmpty {
        timeout_ms: u64,
    },
    Keys(String),
    Paste(String),
    AwaitPaste {
        timeout_ms: u64,
    },
    Submit {
        timeout_ms: u64,
    },
    Pause(u64),
    WalkBackTo {
        question: String,
        tries: usize,
    },
    ExpectQuestion(String),
    WaitFor {
        until: Until,
        timeout_ms: u64,
    },
    SubmitIfReview {
        timeout_ms: u64,
    },
    MarkQueue,
    AwaitPopAll {
        text: String,
        timeout_ms: u64,
    },
    EndUnlessTaken,
    PressPermission {
        decision: Decision,
        expect: Expected,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Until {
    Shows(String),
    ReviewOrGone,
    PermissionGone,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub detach: Vec<Step>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn writes(&self) -> Vec<String> {
        self.steps
            .iter()
            .chain(&self.detach)
            .filter_map(|s| match s {
                Step::Keys(k) => Some(k.clone()),
                Step::Paste(t) => Some(keys::bracketed(t)),
                Step::Submit { .. } => Some(ENTER.to_string()),
                Step::SubmitIfReview { .. } => Some(SUBMIT.to_string()),
                _ => None,
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
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

pub fn reply(text: &str) -> Plan {
    let text = shell::normalize(text);
    Plan {
        steps: vec![
            attach(None),
            Step::RefuseDialog,
            Step::AwaitInput {
                timeout_ms: INPUT_WAIT_MS,
            },
            Step::Keys(keys::clear_input()),
            Step::Pause(KEY_PAUSE_MS),
            Step::RefuseDialog,
            Step::AwaitEmpty {
                timeout_ms: CLEAR_WAIT_MS,
            },
            Step::Paste(text),
            Step::AwaitPaste {
                timeout_ms: PASTE_WAIT_MS,
            },
            Step::Submit {
                timeout_ms: SENT_WAIT_MS,
            },
        ],
        detach: detach(),
    }
}

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
