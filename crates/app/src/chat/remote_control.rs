use notchling_locale as t;

use super::sidebar_node::{node_label, reply_control, Nodes};
use crate::acting::Control;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Reply,
    Stop,
    Answer,
    BringBack,
    Remove,
    Move,
    Terminal,
    Slash,
    Files,
    Opinion,
}

impl Action {
    pub const ALL: [Action; 10] = [
        Action::Reply,
        Action::Stop,
        Action::Answer,
        Action::BringBack,
        Action::Remove,
        Action::Move,
        Action::Terminal,
        Action::Slash,
        Action::Files,
        Action::Opinion,
    ];

    pub fn has_remote_form(self) -> bool {
        matches!(
            self,
            Action::Reply | Action::Stop | Action::Answer | Action::BringBack
        )
    }
}

pub fn remote_control(row_id: &str, nodes: &Nodes, action: Action) -> Option<Control> {
    node_label(row_id, nodes)?;
    if let Some(offline) = reply_control(row_id, nodes) {
        return Some(offline);
    }
    if action.has_remote_form() {
        return None;
    }
    Some(Control {
        enabled: false,
        tooltip: Some(t::cluster::remote_not_here()),
    })
}

pub fn refused(control: Option<Control>) -> bool {
    control.is_some_and(|c| !c.enabled)
}
