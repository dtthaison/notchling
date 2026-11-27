use super::consent_save::Flow;
use super::wire::PendingRequest;
use crate::switches::feed::SwitchFeed;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    #[default]
    Open,
    Refused,
    Done,
}

impl Stage {
    pub fn after(flow: &Flow) -> Stage {
        if flow.closes() {
            Stage::Done
        } else if flow.ended() {
            Stage::Refused
        } else {
            Stage::Open
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cover {
    asked: PendingRequest,
    stage: Stage,
    saving: Option<Vec<String>>,
}

impl Cover {
    pub fn new(asked: PendingRequest) -> Cover {
        Cover {
            asked,
            stage: Stage::Open,
            saving: None,
        }
    }

    pub fn begin(&mut self, feed: &SwitchFeed) -> Vec<String> {
        self.saving = Some(self.asked.twin_ids(&feed.requests()));
        self.pressed()
    }

    pub fn pressed(&self) -> Vec<String> {
        let mut ids = vec![self.asked.id.clone()];
        ids.extend(self.saving.clone().unwrap_or_default());
        ids
    }

    pub fn id(&self) -> &str {
        &self.asked.id
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn covers(&self, request: &PendingRequest) -> bool {
        self.stage == Stage::Open && self.asked.twin_of(request)
    }

    pub fn holds(&self, request: &PendingRequest) -> bool {
        request.id == self.asked.id || self.covers(request)
    }

    pub fn stays(&self, pool: &[PendingRequest]) -> bool {
        match self.stage {
            Stage::Open => self.saving.is_some() || pool.iter().any(|r| self.holds(r)),
            Stage::Refused => true,
            Stage::Done => false,
        }
    }

    pub fn settle(&mut self, stage: Stage, feed: &SwitchFeed) -> Vec<String> {
        let covering = self.stage == Stage::Open;
        let pressed = self.saving.take();
        self.stage = self.stage.max(stage);
        if !covering || self.stage == Stage::Open {
            return Vec::new();
        }
        let mut twins = self.asked.twin_ids(&feed.requests());
        if let Some(pressed) = pressed {
            twins.retain(|id| pressed.contains(id));
        }
        for id in &twins {
            feed.dismiss_request(id);
        }
        twins
    }
}

#[cfg(test)]
#[path = "twins_tests.rs"]
mod tests;
