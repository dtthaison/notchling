use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use notchling_engine::cluster::build_id::build_id;
use notchling_engine::machines::dist_manifest;
use notchling_engine::machines::node_updates::UpdateRow;
use notchling_engine::machines::release_download::this_release;
use notchling_engine::machines::step::StepId;
use notchling_engine::machines::steps_node::dist_dir;

use super::add_parts::AddParts;
use super::model::ClusterModel;
use super::not_joined::{annotated, not_joined, NotJoined};
use super::updates::rows_for;
use crate::chat::data::DataRoot;
use crate::machines::data::{now_secs, Setup};

pub type Offer = (Option<String>, Vec<UpdateRow>);

pub struct AddSource {
    root: Option<DataRoot>,
    parts: OnceLock<Option<AddParts>>,
    fixed_offer: Option<Offer>,
    checked: Mutex<BTreeMap<String, i64>>,
}

impl AddSource {
    pub fn none() -> AddSource {
        AddSource {
            root: None,
            parts: OnceLock::new(),
            fixed_offer: None,
            checked: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn for_root(root: DataRoot) -> AddSource {
        AddSource {
            root: Some(root),
            parts: OnceLock::new(),
            fixed_offer: None,
            checked: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn with_offer(mut self, offer: Offer) -> AddSource {
        self.fixed_offer = Some(offer);
        self
    }

    pub fn with_checked(self, name: &str, at_s: i64) -> AddSource {
        self.stamp(name, at_s);
        self
    }

    fn stamp(&self, name: &str, at_s: i64) {
        self.checked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.to_owned(), at_s);
    }

    pub fn note_checked(&self, name: &str) {
        self.stamp(name, now_secs() as i64);
    }

    pub fn check(&self, name: &str) {
        let Some(parts) = self.parts() else {
            return;
        };
        parts.engine.refresh(Some(&[name.to_owned()]), true);
        self.note_checked(name);
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
        let list = not_joined(&machines, &joined, model);
        let reach = |name: &str| parts.engine.results(name).get(&StepId::REACHABLE).cloned();
        let stamps = self.checked.lock().unwrap_or_else(|e| e.into_inner());
        annotated(list, &reach, &|name| stamps.get(name).copied())
    }
}

impl AddSource {
    pub fn updates(&self, model: &ClusterModel) -> Offer {
        if let Some(fixed) = &self.fixed_offer {
            return fixed.clone();
        }
        let this_build = build_id().map(str::to_owned);
        let Some(parts) = self.parts() else {
            return (this_build, Vec::new());
        };
        let manifest = self
            .root
            .as_ref()
            .and_then(|root| dist_manifest::read(&dist_dir(&root.data_dir)));
        let machines = parts.registry.all();
        let node_of = |name: &str| {
            parts
                .engine
                .results(name)
                .get(&StepId::NODE_JOINED)
                .and_then(|r| r.detected.node.clone())
        };
        let failed = parts.engine.binary_failures();
        let rows = rows_for(
            this_build.as_deref(),
            model,
            &machines,
            &node_of,
            manifest.as_ref(),
            &failed,
            this_release().is_some(),
        );
        (this_build, rows)
    }
}
