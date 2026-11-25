use super::decide::{want, Kind, Setting};

pub trait Power: Send {
    fn hold(&mut self, kind: Kind) -> Result<Box<dyn Send>, String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Unchanged,
    Held(Kind),
    Released,
    Failed(String),
}

pub struct Awake {
    power: Box<dyn Power>,
    held: Option<(Kind, Box<dyn Send>)>,
    refusal: Option<String>,
}

impl Awake {
    pub fn new(power: Box<dyn Power>) -> Awake {
        Awake {
            power,
            held: None,
            refusal: None,
        }
    }

    pub fn holding(&self) -> Option<Kind> {
        self.held.as_ref().map(|(kind, _)| *kind)
    }

    pub fn apply(&mut self, setting: Setting, busy: impl FnOnce() -> bool) -> Outcome {
        match (want(setting, busy), self.holding()) {
            (None, _) => self.release(),
            (Some(wanted), Some(held)) if wanted == held => Outcome::Unchanged,
            (Some(wanted), _) => self.take(wanted),
        }
    }

    pub fn release(&mut self) -> Outcome {
        self.refusal = None;
        match self.held.take() {
            Some(_) => Outcome::Released,
            None => Outcome::Unchanged,
        }
    }

    fn take(&mut self, kind: Kind) -> Outcome {
        match self.power.hold(kind) {
            Ok(guard) => {
                self.refusal = None;
                self.held = Some((kind, guard));
                Outcome::Held(kind)
            }
            Err(why) => {
                if self.refusal.as_deref() == Some(why.as_str()) {
                    Outcome::Unchanged
                } else {
                    self.refusal = Some(why.clone());
                    Outcome::Failed(why)
                }
            }
        }
    }
}
