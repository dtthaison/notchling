use std::sync::{Mutex, MutexGuard};

#[cfg(any(test, target_os = "macos"))]
use notchling_engine::machines::engine::SetupEngine;
#[cfg(any(test, target_os = "macos"))]
use notchling_engine::machines::engine_plan::{PlanKind, ShownPlan};
use notchling_engine::machines::step::FixOutcome;
#[cfg(any(test, target_os = "macos"))]
use notchling_locale as t;

use super::updates::NotNow;

#[derive(Default)]
struct Inner {
    hidden: NotNow,
    running: bool,
    outcome: Option<FixOutcome>,
}

#[derive(Default)]
pub struct UpdateSession {
    inner: Mutex<Inner>,
}

impl UpdateSession {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn hide(&self, this_build: Option<&str>) {
        self.inner().hidden.hide(this_build);
    }

    pub fn hides(&self, this_build: Option<&str>) -> bool {
        self.inner().hidden.hides(this_build)
    }

    pub fn running(&self) -> bool {
        self.inner().running
    }

    pub fn outcome(&self) -> Option<FixOutcome> {
        self.inner().outcome.clone()
    }

    pub fn dismissed(&self) {
        let mut inner = self.inner();
        if !inner.running {
            inner.outcome = None;
        }
    }

    pub fn begin(&self) -> bool {
        let mut inner = self.inner();
        if inner.running {
            return false;
        }
        inner.running = true;
        inner.outcome = None;
        true
    }

    pub fn finish(&self, outcome: FixOutcome) {
        let mut inner = self.inner();
        inner.running = false;
        inner.outcome = Some(outcome);
    }
}

#[cfg(any(test, target_os = "macos"))]
#[derive(Default)]
pub struct Offered {
    names: Option<Vec<String>>,
    shown: Option<ShownPlan>,
    error: Option<String>,
}

#[cfg(any(test, target_os = "macos"))]
impl Offered {
    pub fn sync_names(&mut self, engine: &SetupEngine, names: Vec<String>) -> bool {
        if self.names.as_ref() == Some(&names) {
            return false;
        }
        self.cancel(engine);
        let planned = if names.is_empty() {
            Err(t::cluster::update_nothing().to_owned())
        } else {
            engine.plan("", PlanKind::UpdateNotchling(names.clone()))
        };
        match planned {
            Ok(shown) => {
                self.shown = Some(shown);
                self.error = None;
            }
            Err(why) => {
                self.shown = None;
                self.error = Some(why);
            }
        }
        self.names = Some(names);
        true
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn names(&self) -> Vec<String> {
        match self.shown.as_ref().map(|p| &p.kind) {
            Some(PlanKind::UpdateNotchling(names)) => names.clone(),
            _ => Vec::new(),
        }
    }

    #[cfg(target_os = "macos")]
    pub fn admin_machines(&self) -> Vec<String> {
        match &self.shown {
            Some(ShownPlan {
                kind: PlanKind::UpdateNotchling(names),
                steps,
                ..
            }) => names
                .iter()
                .zip(steps)
                .filter(|(_, step)| step.needs_admin)
                .map(|(name, _)| name.clone())
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn reset(&mut self, engine: Option<&SetupEngine>) {
        match engine {
            Some(engine) => self.cancel(engine),
            None => self.shown = None,
        }
        self.names = None;
        self.error = None;
    }

    pub fn cancel(&mut self, engine: &SetupEngine) {
        if let Some(old) = self.shown.take() {
            engine.cancel(old.id);
        }
    }

    pub fn take(&mut self) -> Option<ShownPlan> {
        self.shown.take()
    }

    pub fn shown(&self) -> Option<&ShownPlan> {
        self.shown.as_ref()
    }

    #[cfg(test)]
    pub fn button(&self) -> (String, bool) {
        let names = self.names();
        let first = names.first().map(String::as_str).unwrap_or_default();
        (
            t::cluster::update_button_run(names.len(), first),
            self.shown.is_some(),
        )
    }
}
