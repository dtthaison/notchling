//! What the notch draws, to tell whether a poll changed it: [`NotchModel::drawn`] and [`Drawn`].
//! A file of its own because `model.rs` is near its size limit.

use super::layout::{NotchState, NotchWidth, Size};
use super::model::{ago, NotchModel};
use super::requests::HookRequest;
use notchling_engine::sessions::{Agent, AgentGroup};
use notchling_engine::transcript::snapshot::Snapshot;

/// Everything the notch draws from the model, to compare two polls: the view draws again only
/// when this differs. The ages are the strings the rows show (`ago`), so a clock that moved but
/// left every row's age as it was draws nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    agents: Vec<Agent>,
    snapshots: Vec<(String, Snapshot)>,
    requests: Vec<(String, HookRequest)>,
    state: NotchState,
    tab: AgentGroup,
    notch: Size,
    width: NotchWidth,
    show_last_reply: bool,
    daemon_note: Option<&'static str>,
    answered: Vec<String>,
    card_height: Option<f64>,
    inbox_unread: usize,
    ages: Vec<String>,
}

impl NotchModel {
    /// What the notch would draw now.
    pub fn drawn(&self) -> Drawn {
        let mut snapshots: Vec<_> = self
            .snapshots
            .iter()
            .map(|(id, s)| (id.clone(), s.clone()))
            .collect();
        snapshots.sort_by(|a, b| a.0.cmp(&b.0));
        let mut requests: Vec<_> = self
            .requests
            .iter()
            .map(|(id, r)| (id.clone(), r.clone()))
            .collect();
        requests.sort_by(|a, b| a.0.cmp(&b.0));
        let mut answered: Vec<_> = self.answered.iter().cloned().collect();
        answered.sort();
        Drawn {
            agents: self.agents.clone(),
            snapshots,
            requests,
            state: self.state.clone(),
            tab: self.tab,
            notch: self.notch,
            width: self.width,
            show_last_reply: self.show_last_reply,
            daemon_note: self.daemon_note,
            answered,
            card_height: self.card_height,
            inbox_unread: self.inbox_unread,
            ages: self
                .agents
                .iter()
                .map(|a| ago(a.since_ms, self.now_ms))
                .collect(),
        }
    }
}

#[cfg(test)]
#[path = "drawn_tests.rs"]
mod tests;
