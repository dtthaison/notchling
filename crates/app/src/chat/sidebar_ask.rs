use notchling_kit::Status;

use super::asks::AskFeed;
use super::data::Session;
use super::sidebar::SidebarModel;

impl SidebarModel {
    pub fn with_asks(mut self, feed: &AskFeed, sessions: &[Session]) -> SidebarModel {
        for row in self.sections.iter_mut().flat_map(|s| &mut s.rows) {
            let asking = sessions
                .iter()
                .find(|s| s.agent.id == row.id)
                .is_some_and(|s| feed.needs_you(&s.agent));
            if asking {
                row.status = Status::Needs;
            }
        }
        self
    }
}

#[cfg(test)]
#[path = "sidebar_ask_tests.rs"]
mod tests;
