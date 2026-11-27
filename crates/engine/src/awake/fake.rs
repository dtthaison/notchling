use super::decide::Kind;
use super::hold::Power;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct FakePower {
    log: Arc<Mutex<Vec<String>>>,
    refuse: Arc<Mutex<Option<String>>>,
}

struct FakeGuard {
    name: &'static str,
    log: Arc<Mutex<Vec<String>>>,
}

impl Drop for FakeGuard {
    fn drop(&mut self) {
        self.log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(format!("release {}", self.name));
    }
}

fn name(kind: Kind) -> &'static str {
    match kind {
        Kind::System => "system",
        Kind::Display => "display",
    }
}

impl FakePower {
    pub fn new() -> FakePower {
        FakePower::default()
    }

    pub fn events(&self) -> Vec<String> {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn refuse_with(&self, why: Option<&str>) {
        *self.refuse.lock().unwrap_or_else(|e| e.into_inner()) = why.map(String::from);
    }
}

impl Power for FakePower {
    fn hold(&mut self, kind: Kind) -> Result<Box<dyn Send>, String> {
        let refusal = self
            .refuse
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut log = self.log.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(why) = refusal {
            log.push(format!("refused {}", name(kind)));
            return Err(why);
        }
        log.push(format!("hold {}", name(kind)));
        Ok(Box::new(FakeGuard {
            name: name(kind),
            log: self.log.clone(),
        }))
    }
}
