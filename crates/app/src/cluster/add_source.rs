use std::sync::OnceLock;

use notchling_engine::machines::step::StepId;

use super::add_parts::AddParts;
use super::model::ClusterModel;
use super::not_joined::{not_joined, NotJoined};
use crate::chat::data::DataRoot;
use crate::machines::data::Setup;

pub struct AddSource {
    root: Option<DataRoot>,
    parts: OnceLock<Option<AddParts>>,
}

impl AddSource {
    pub fn none() -> AddSource {
        AddSource {
            root: None,
            parts: OnceLock::new(),
        }
    }

    pub fn for_root(root: DataRoot) -> AddSource {
        AddSource {
            root: Some(root),
            parts: OnceLock::new(),
        }
    }

    pub fn over(parts: AddParts) -> AddSource {
        let source = AddSource::none();
        source.parts.set(Some(parts)).ok();
        source
    }

    pub fn parts(&self) -> Option<AddParts> {
        self.parts
            .get_or_init(|| {
                let root = self.root.clone()?;
                Some(AddParts::live(&Setup::for_root(root, true, false)))
            })
            .clone()
    }

    pub fn names(&self) -> Vec<String> {
        self.parts().map(|p| p.names()).unwrap_or_default()
    }

    pub fn not_joined(&self, model: &ClusterModel) -> Vec<NotJoined> {
        let Some(parts) = self.parts() else {
            return Vec::new();
        };
        let machines = parts.registry.all();
        let joined = |name: &str| {
            parts
                .engine
                .results(name)
                .get(&StepId::NODE_JOINED)
                .cloned()
        };
        not_joined(&machines, &joined, model)
    }
}
