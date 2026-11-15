use std::collections::BTreeMap;

use notchling_locale as t;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Prepare,
    CatchUp,
    Freeze,
    FinalSnapshot,
    StopOld,
    Switch,
    Resume,
    Announce,
    Done,
    Undone,
}

pub const RUN: [Step; 9] = [
    Step::Prepare,
    Step::CatchUp,
    Step::Freeze,
    Step::FinalSnapshot,
    Step::StopOld,
    Step::Switch,
    Step::Resume,
    Step::Announce,
    Step::Done,
];

impl Step {
    pub fn from_name(name: &str) -> Option<Step> {
        Some(match name {
            "prepare" => Step::Prepare,
            "catch_up" => Step::CatchUp,
            "freeze" => Step::Freeze,
            "final_snapshot" => Step::FinalSnapshot,
            "stop_old" => Step::StopOld,
            "switch" => Step::Switch,
            "resume" => Step::Resume,
            "announce" => Step::Announce,
            "done" => Step::Done,
            "undone" => Step::Undone,
            _ => return None,
        })
    }

    pub fn finished(self) -> bool {
        matches!(self, Step::Done | Step::Undone)
    }

    fn position(self) -> usize {
        RUN.iter().position(|s| *s == self).unwrap_or(RUN.len())
    }

    pub fn line(self, name: &str) -> String {
        match self {
            Step::Prepare => t::cluster::move_step_prepare(name),
            Step::CatchUp => t::cluster::move_step_catch_up(name),
            Step::Freeze => t::cluster::move_step_freeze().to_owned(),
            Step::FinalSnapshot => t::cluster::move_step_final().to_owned(),
            Step::StopOld => t::cluster::move_step_stop().to_owned(),
            Step::Switch => t::cluster::move_step_switch(name),
            Step::Resume => t::cluster::move_step_resume(name),
            Step::Announce => t::cluster::move_step_announce().to_owned(),
            Step::Done | Step::Undone => t::cluster::move_step_done(name),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveStatus {
    pub to: String,
    pub step: Step,
    pub moving: Vec<String>,
    pub staying: Vec<(String, String)>,
    pub cut: Vec<String>,
    pub stopped: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub reason: Option<String>,
}

fn texts(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|all| {
            all.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn pairs(value: &Value, key: &str) -> Vec<(String, String)> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|all| {
            all.iter()
                .filter_map(|pair| {
                    let pair = pair.as_array()?;
                    Some((
                        pair.first()?.as_str()?.to_owned(),
                        pair.get(1)?.as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl MoveStatus {
    pub fn of(status: &Value) -> Option<MoveStatus> {
        let moved = status.get("move").filter(|m| m.is_object())?;
        Some(MoveStatus {
            to: moved.get("to")?.as_str()?.to_owned(),
            step: Step::from_name(moved.get("step")?.as_str()?)?,
            moving: texts(moved, "moving"),
            staying: pairs(moved, "staying"),
            cut: texts(moved, "cut"),
            stopped: texts(moved, "stopped"),
            failed: pairs(moved, "failed"),
            reason: moved
                .get("reason")
                .and_then(Value::as_str)
                .filter(|r| !r.is_empty())
                .map(str::to_owned),
        })
    }

    pub fn running(&self) -> bool {
        !self.step.finished()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Confirm,
    Starting,
    Refused(String),
    Running,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Start,
    Cancel,
    MoveNow,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepState {
    Done,
    Current,
    Waiting,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionLine {
    pub name: String,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepLine {
    pub text: String,
    pub state: StepState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub text: String,
    pub problem: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveView {
    pub title: String,
    pub intro: Option<String>,
    pub moving: Vec<SessionLine>,
    pub staying: Vec<SessionLine>,
    pub steps: Vec<StepLine>,
    pub notes: Vec<Note>,
    pub buttons: Vec<Button>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveSheet {
    pub to: String,
    pub to_name: String,
    pub phase: Phase,
    pub failure: Option<String>,
}

pub type Names = BTreeMap<String, String>;

fn shown(names: &Names, id: &str) -> String {
    names
        .get(id)
        .filter(|n| !n.is_empty())
        .cloned()
        .unwrap_or_else(|| id.chars().take(8).collect())
}

fn list(names: &Names, ids: &[String]) -> String {
    ids.iter()
        .map(|id| shown(names, id))
        .collect::<Vec<_>>()
        .join(", ")
}

impl MoveSheet {
    pub fn new(to: &str, to_name: &str) -> MoveSheet {
        MoveSheet {
            to: to.to_owned(),
            to_name: to_name.to_owned(),
            phase: Phase::Confirm,
            failure: None,
        }
    }

    pub fn running(to: &str, to_name: &str) -> MoveSheet {
        MoveSheet {
            phase: Phase::Running,
            ..MoveSheet::new(to, to_name)
        }
    }

    pub fn view(&self, status: Option<&MoveStatus>, names: &Names) -> MoveView {
        let mut view = MoveView {
            title: t::cluster::move_title(&self.to_name),
            intro: None,
            moving: Vec::new(),
            staying: Vec::new(),
            steps: Vec::new(),
            notes: Vec::new(),
            buttons: Vec::new(),
        };
        match &self.phase {
            Phase::Confirm => {
                view.intro = Some(t::cluster::move_intro(&self.to_name));
                view.buttons = vec![Button::Start, Button::Cancel];
            }
            Phase::Starting => {
                view.intro = Some(t::cluster::move_starting(&self.to_name));
            }
            Phase::Refused(reason) => {
                view.notes.push(Note {
                    text: reason.clone(),
                    problem: true,
                });
                view.buttons = vec![Button::Close];
            }
            Phase::Running => {
                let Some(status) = status else {
                    view.intro = Some(t::cluster::move_starting(&self.to_name));
                    view.buttons = vec![Button::Close];
                    return view;
                };
                self.fill(&mut view, status, names);
            }
        }
        view
    }

    fn fill(&self, view: &mut MoveView, status: &MoveStatus, names: &Names) {
        let session = |id: &String, note: Option<String>| SessionLine {
            name: shown(names, id),
            note,
        };
        view.moving = status.moving.iter().map(|id| session(id, None)).collect();
        view.staying = status
            .staying
            .iter()
            .map(|(id, why)| session(id, Some(why.clone())))
            .collect();
        if status.step == Step::Undone {
            let why = status.reason.clone().unwrap_or_default();
            view.notes.push(Note {
                text: t::cluster::move_undone(&why),
                problem: true,
            });
        } else {
            let at = status.step.position();
            view.steps = RUN
                .iter()
                .enumerate()
                .map(|(index, step)| StepLine {
                    text: step.line(&self.to_name),
                    state: if index < at || status.step == Step::Done {
                        StepState::Done
                    } else if index == at {
                        StepState::Current
                    } else {
                        StepState::Waiting
                    },
                })
                .collect();
        }
        if !status.cut.is_empty() {
            view.notes.push(Note {
                text: t::cluster::move_cut(&list(names, &status.cut)),
                problem: false,
            });
        }
        for (id, why) in &status.failed {
            view.notes.push(Note {
                text: t::cluster::move_failed(&shown(names, id), why),
                problem: true,
            });
        }
        if let Some(error) = &self.failure {
            view.notes.push(Note {
                text: t::cluster::move_now_failed(error),
                problem: true,
            });
        }
        if status.step == Step::Freeze {
            view.buttons.push(Button::MoveNow);
        }
        view.buttons.push(Button::Close);
    }
}
