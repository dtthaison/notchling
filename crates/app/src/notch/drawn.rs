use super::ask::AskCard;
use super::layout::{NotchState, NotchWidth, Size};
use super::model::{ago, NotchModel};
use super::requests::HookRequest;
use notchling_engine::sessions::{Agent, AgentGroup};
use notchling_engine::transcript::snapshot::Snapshot;

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
    message_needs: Vec<String>,
    asks: Vec<(String, AskCard)>,
    ages: Vec<String>,
}

impl NotchModel {
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
        let mut message_needs: Vec<_> = self.message_needs.iter().cloned().collect();
        message_needs.sort();
        let mut asks: Vec<_> = self
            .ask_cards
            .iter()
            .map(|(id, c)| (id.clone(), c.clone()))
            .collect();
        asks.sort_by(|a, b| a.0.cmp(&b.0));
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
            message_needs,
            asks,
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
