use std::sync::Arc;

use notchling_engine::machines::engine::SetupEngine;

use crate::machines::data::Setup;
use crate::machines::model::{Parts, Registry};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub user: String,
    pub home: String,
}

#[derive(Clone)]
pub struct AddParts {
    pub engine: Arc<SetupEngine>,
    pub registry: Arc<dyn Registry>,
    pub this: Account,
    pub off: Option<&'static str>,
}

impl AddParts {
    pub fn from_machines(parts: &Parts, this: Account) -> AddParts {
        AddParts {
            engine: parts.engine.clone(),
            registry: parts.registry.clone(),
            this,
            off: parts.off,
        }
    }

    pub fn live(setup: &Setup) -> AddParts {
        AddParts::from_machines(&setup.parts(), this_mac(setup))
    }

    pub fn names(&self) -> Vec<String> {
        self.registry.all().into_iter().map(|m| m.name).collect()
    }
}

fn this_mac(setup: &Setup) -> Account {
    let home = setup.home.to_string_lossy().into_owned();
    let user = std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .or_else(|| {
            setup
                .home
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    Account { user, home }
}
