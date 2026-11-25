use notchling_locale as t;
use serde_json::Value;

use super::model::ClusterModel;
use super::move_sheet::{Button, MoveView, Note, StepLine, StepState};
use super::promote::{PromoteStatus, PromoteStep, RUN};
use super::wire::Request;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromotePhase {
    Confirm,
    Starting,
    Risk(String),
    Relayed(String),
    Running,
    Refused(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromoteSheet {
    pub to: String,
    pub to_name: String,
    pub primary_name: String,
    pub phase: PromotePhase,
    before: Option<i64>,
}

impl PromoteSheet {
    pub fn new(to: &str, to_name: &str, primary_name: &str) -> PromoteSheet {
        PromoteSheet {
            to: to.to_owned(),
            to_name: to_name.to_owned(),
            primary_name: primary_name.to_owned(),
            phase: PromotePhase::Confirm,
            before: None,
        }
    }

    pub fn running(to: &str, to_name: &str, primary_name: &str) -> PromoteSheet {
        PromoteSheet {
            phase: PromotePhase::Running,
            ..PromoteSheet::new(to, to_name, primary_name)
        }
    }

    pub fn waiting(&self) -> bool {
        self.phase == PromotePhase::Starting
    }

    pub fn start(&mut self, model: &ClusterModel) -> Option<Request> {
        let confirmed = match self.phase {
            PromotePhase::Confirm => false,
            PromotePhase::Risk(_) => true,
            _ => return None,
        };
        let Some(request) = model.promote_request(&self.to, confirmed) else {
            self.phase = PromotePhase::Refused(t::cluster::move_unavailable().to_owned());
            return None;
        };
        self.before = model.promote_status().map(|p| p.started);
        self.phase = PromotePhase::Starting;
        Some(request)
    }

    pub fn answered(&mut self, reply: Result<Value, String>) {
        self.phase = match reply {
            Err(error) => PromotePhase::Refused(error),
            Ok(reply) => {
                let said = |key: &str| reply.get(key).and_then(Value::as_str).map(str::to_owned);
                if reply.get("relayed").and_then(Value::as_bool) == Some(true) {
                    PromotePhase::Relayed(
                        said("text")
                            .filter(|t| !t.is_empty())
                            .unwrap_or_else(|| t::cluster::promote_nothing_moved().to_owned()),
                    )
                } else if let Some(risk) = said("needsConfirm") {
                    PromotePhase::Risk(risk)
                } else {
                    PromotePhase::Running
                }
            }
        };
    }

    fn progress<'a>(&self, model: &'a ClusterModel) -> Option<&'a PromoteStatus> {
        model
            .promote_status()
            .filter(|p| p.node == self.to && Some(p.started) != self.before)
    }

    fn steps(&self, at: Option<usize>, all: bool) -> Vec<StepLine> {
        RUN.iter()
            .enumerate()
            .map(|(index, step)| StepLine {
                text: step.line(&self.to_name, &self.primary_name),
                state: match at {
                    _ if all => StepState::Done,
                    Some(at) if index < at => StepState::Done,
                    Some(at) if index == at => StepState::Current,
                    _ => StepState::Waiting,
                },
            })
            .collect()
    }

    fn fill_running(&self, view: &mut MoveView, model: &ClusterModel) {
        let done = model.row(&self.to).is_some_and(|r| r.primary);
        match self.progress(model) {
            Some(p) => match p.step {
                PromoteStep::Failed => view.notes.push(Note {
                    text: t::cluster::move_undone(p.error.as_deref().unwrap_or_default()),
                    problem: true,
                }),
                PromoteStep::Relayed => view.notes.push(Note {
                    text: p
                        .text
                        .clone()
                        .unwrap_or_else(|| t::cluster::promote_nothing_moved().to_owned()),
                    problem: false,
                }),
                PromoteStep::Done => view.steps = self.steps(None, true),
                step => match step.position() {
                    Some(at) => view.steps = self.steps(Some(at), false),
                    None => view.intro = Some(t::cluster::promote_step_unknown().to_owned()),
                },
            },
            None if done => view.steps = self.steps(None, true),
            None => view.intro = Some(t::cluster::promote_step_unknown().to_owned()),
        }
    }

    pub fn view(&self, model: &ClusterModel) -> MoveView {
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
            PromotePhase::Confirm => {
                view.intro = Some(t::cluster::promote_intro(&self.primary_name, &self.to_name));
                view.buttons = vec![Button::Start, Button::Cancel];
            }
            PromotePhase::Starting => {
                view.intro = Some(t::cluster::promote_asking(&self.to_name));
            }
            PromotePhase::Risk(text) => {
                view.notes.push(Note {
                    text: text.clone(),
                    problem: true,
                });
                view.buttons = vec![Button::Anyway, Button::Cancel];
            }
            PromotePhase::Relayed(text) => {
                view.notes.push(Note {
                    text: text.clone(),
                    problem: false,
                });
                view.buttons = vec![Button::Close];
            }
            PromotePhase::Refused(reason) => {
                view.notes.push(Note {
                    text: reason.clone(),
                    problem: true,
                });
                view.buttons = vec![Button::Close];
            }
            PromotePhase::Running => {
                self.fill_running(&mut view, model);
                view.buttons = vec![Button::Close];
            }
        }
        view
    }
}
